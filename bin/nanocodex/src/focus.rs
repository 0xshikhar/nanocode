use nanocode::{
    agent::events::{AgentEvent, AgentEventKind},
    durability::TranscriptItem,
    subagents::{AgentDescriptor, AgentId, MessagePriority},
};
use nanocode_tui::transcript::{NoticeLevel, Transcript, TranscriptPolicy};

use crate::{
    app::{App, AppEvent, DIFF_PREVIEW_LINES, TOOL_PREVIEW_LINES},
    events::{EventMapper, subagent_prompt_display},
};

pub struct SubagentView {
    pub role: String,
    pub model: Option<String>,
    pub transcript: Transcript,
    mapper: EventMapper,
    loading: bool,
}

pub struct FocusReady {
    id: AgentId,
    role: String,
    model: Option<String>,
    history: Vec<TranscriptItem>,
}

impl SubagentView {
    fn new(role: String, model: Option<String>) -> Self {
        Self {
            role,
            model,
            transcript: Transcript::new(TranscriptPolicy {
                collapsed_preview_lines: TOOL_PREVIEW_LINES,
                collapsed_diff_lines: DIFF_PREVIEW_LINES,
            }),
            mapper: EventMapper::subagent(),
            loading: false,
        }
    }
}

impl App {
    pub(crate) fn track_subagent(&mut self, descriptor: &AgentDescriptor) {
        self.views.entry(descriptor.id).or_insert_with(|| {
            SubagentView::new(
                descriptor.role.clone(),
                descriptor.model.as_ref().map(ToString::to_string),
            )
        });
    }

    pub(crate) fn subagent_event(&mut self, id: AgentId, event: &AgentEvent) {
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        if event.kind == AgentEventKind::RunStarted
            && let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload.get())
            && let Some(model) = payload["model"].as_str()
        {
            view.model = Some(model.to_owned());
        }
        drop(view.mapper.apply(&mut view.transcript, event));
    }

    pub(crate) fn visible_transcript(&mut self) -> &mut Transcript {
        match self.focus.and_then(|id| self.views.get_mut(&id)) {
            Some(view) => &mut view.transcript,
            None => &mut self.transcript,
        }
    }

    pub(crate) fn reset_focus(&mut self) {
        self.focus = None;
        self.views.clear();
    }

    pub(crate) fn focus_command(&mut self, argument: &str) {
        let argument = argument.trim().trim_start_matches('#');
        if argument.is_empty() || argument.eq_ignore_ascii_case("main") {
            self.unfocus();
            return;
        }
        let id = argument.parse::<u64>().ok().map(AgentId::new).or_else(|| {
            self.views
                .iter()
                .filter(|(_, view)| view.role.eq_ignore_ascii_case(argument))
                .map(|(id, _)| *id)
                .min()
        });
        match id {
            Some(id) => self.focus_on(id),
            None => self.flash(format!("no subagent named {argument}; use its #id")),
        }
    }

    pub(crate) fn cycle_focus(&mut self) {
        let order = self
            .tree
            .ordered()
            .into_iter()
            .filter_map(|(_, _, node)| node.id.parse::<u64>().ok().map(AgentId::new))
            .collect::<Vec<_>>();
        let next = match self.focus {
            None => order.first().copied(),
            Some(current) => order
                .iter()
                .position(|id| *id == current)
                .and_then(|index| order.get(index + 1))
                .copied(),
        };
        match next {
            Some(id) => self.focus_on(id),
            None if self.focus.is_some() => self.unfocus(),
            None => self.flash("no subagents to focus"),
        }
    }

    fn unfocus(&mut self) {
        if self.focus.take().is_some() {
            self.flash("back to the main thread");
        }
    }

    fn focus_on(&mut self, id: AgentId) {
        let Some(root) = self
            .agent
            .as_ref()
            .map(|agent| agent.session_id().to_owned())
        else {
            self.flash("no session yet");
            return;
        };
        let Some(control) = self.host.runtime.subagents().cloned() else {
            self.flash("subagents are disabled for this session");
            return;
        };
        let known = self.views.get_mut(&id).map(|view| {
            let empty = view.transcript.is_empty() && !view.loading;
            view.loading |= empty;
            (view.role.clone(), empty)
        });
        let backfill = match known {
            Some((role, empty)) => {
                self.focus = Some(id);
                self.flash(format!(
                    "focused #{id} {role} · enter messages it · esc esc interrupts · /agent returns"
                ));
                empty
            }
            None => true,
        };
        if !backfill {
            return;
        }
        let store = self.host.config.store.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let ready = async {
                let entry = control
                    .agents(&root)
                    .await
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|entry| entry.agent_id == id)
                    .ok_or_else(|| format!("no subagent #{id} in this session"))?;
                let history = match store {
                    Some(store) => {
                        let session = control
                            .session_id(&root, id)
                            .await
                            .map_err(|error| error.to_string())?;
                        store
                            .load(&session)
                            .await
                            .map_err(|error| error.to_string())?
                            .transcript
                    }
                    None => Vec::new(),
                };
                Ok::<_, String>(FocusReady {
                    id,
                    role: entry.role,
                    model: entry.model.as_ref().map(ToString::to_string),
                    history,
                })
            };
            drop(tx.send(match ready.await {
                Ok(ready) => AppEvent::FocusReady(Box::new(ready)),
                Err(error) => AppEvent::Notice(NoticeLevel::Error, format!("focus #{id}: {error}")),
            }));
        });
    }

    pub(crate) fn focus_ready(&mut self, ready: FocusReady) {
        let FocusReady {
            id,
            role,
            model,
            history,
        } = ready;
        let view = self
            .views
            .entry(id)
            .or_insert_with(|| SubagentView::new(role, model));
        view.loading = false;
        if view.transcript.is_empty() {
            crate::sessions::replay(
                &mut view.transcript,
                history
                    .into_iter()
                    .map(|item| match item {
                        TranscriptItem::User(text) => {
                            TranscriptItem::User(subagent_prompt_display(&text))
                        }
                        other => other,
                    })
                    .collect(),
            );
        }
        if self.focus != Some(id) {
            self.focus = Some(id);
            let role = view.role.clone();
            self.flash(format!(
                "focused #{id} {role} · enter messages it · esc esc interrupts · /agent returns"
            ));
        }
    }

    pub(crate) fn message_focused(&mut self, id: AgentId, text: String) {
        let (Some(root), Some(control)) = (
            self.agent
                .as_ref()
                .map(|agent| agent.session_id().to_owned()),
            self.host.runtime.subagents().cloned(),
        ) else {
            self.flash("no session to message");
            return;
        };
        if let Some(view) = self.views.get_mut(&id) {
            view.transcript.scroll_to_bottom();
        }
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(error) = control
                .send_user_message(&root, id, MessagePriority::Urgent, text)
                .await
            {
                drop(tx.send(AppEvent::Notice(
                    NoticeLevel::Error,
                    format!("message to #{id} failed: {error}"),
                )));
            }
        });
    }

    pub(crate) fn interrupt_focused(&mut self, id: AgentId) {
        let (Some(root), Some(control)) = (
            self.agent
                .as_ref()
                .map(|agent| agent.session_id().to_owned()),
            self.host.runtime.subagents().cloned(),
        ) else {
            return;
        };
        self.flash(format!("interrupting #{id}"));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(error) = control.interrupt(&root, id).await {
                drop(tx.send(AppEvent::Notice(
                    NoticeLevel::Error,
                    format!("interrupt #{id} failed: {error}"),
                )));
            }
        });
    }
}
