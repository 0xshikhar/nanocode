use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use nanocode::{
    AgentStatus, HarnessFamily, ModelSelection, PromptRequest, PromptRoute, ScopedAgentUpdate,
    ServiceTier, Thinking, TurnControl, TurnResult,
    agent::{
        Agent, AgentEvents,
        events::{AgentEvent, AgentEventKind},
    },
    durability::{SessionSummary, TranscriptItem},
};
use nanocode_tui::{
    composer::{Composer, ComposerAction},
    crossterm::event::{
        Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    history::History,
    picker::{Picker, PickerAction, PickerItem},
    queue::{Queue, QueueKind, QueueState, QueuedPrompt},
    scheduler::RenderScheduler,
    spinner::Spinner,
    terminal::TerminalSession,
    theme::Theme,
    transcript::{CellKind, NoticeLevel, ToolStatus, Transcript, TranscriptPolicy},
    tree::{AgentTree, NodeStatus},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    events::{Accepted, EventMapper},
    host::{Host, Result},
    models, view,
};

pub const DISPLAY_FRAME: Duration = Duration::from_micros(16_667);
pub const SPINNER_FRAME: Duration = Duration::from_millis(80);
pub const QUIT_CONFIRMATION: Duration = Duration::from_secs(2);
pub const TOOL_PREVIEW_LINES: usize = 6;
pub const DIFF_PREVIEW_LINES: usize = 40;

pub enum AppEvent {
    Agent {
        generation: u64,
        event: AgentEvent,
    },
    TurnStarted {
        generation: u64,
        control: TurnControl,
    },
    RoutedTurn {
        generation: u64,
        id: String,
        control: TurnControl,
    },
    TurnFinished {
        generation: u64,
        result: std::result::Result<TurnResult, String>,
    },
    PromptFailed {
        generation: u64,
        error: String,
        text: String,
    },
    SteerSent {
        id: String,
        result: std::result::Result<(), String>,
    },
    AgentReady(Box<Ready>),
    AgentFailed(String),
    Sessions(std::result::Result<Vec<SessionSummary>, String>),
    Btw {
        id: u64,
        update: BtwUpdate,
    },
    Subagent(ScopedAgentUpdate),
    Notice(NoticeLevel, String),
    Applied(std::result::Result<(ModelSelection, Thinking, ServiceTier, String), String>),
}

pub enum BtwUpdate {
    Delta(String),
    Message(String),
    Done,
    Failed(String),
}

pub struct Ready {
    pub agent: Agent,
    pub events: AgentEvents,
    pub model: ModelSelection,
    pub thinking: Thinking,
    pub tier: ServiceTier,
    pub host: Option<Host>,
    pub history: Option<Vec<TranscriptItem>>,
    pub notice: String,
    pub reset_transcript: bool,
}

pub enum TurnState {
    Idle,
    Starting(Instant),
    External(Instant),
    Running {
        control: TurnControl,
        since: Instant,
    },
}

pub enum PickerPurpose {
    Model,
    Thinking(ModelSelection),
    Tier(ModelSelection, Thinking),
    Session(Vec<SessionSummary>),
    Recent(Vec<String>),
}

pub enum Overlay {
    Picker(PickerPurpose, Picker),
    Help,
}

pub struct Launch {
    pub host: Host,
    pub model: ModelSelection,
    pub thinking: Thinking,
    pub tier: ServiceTier,
    pub resume: Option<String>,
    pub pick_session: bool,
    pub prompt: Option<String>,
    pub remote_label: Option<String>,
    pub history_path: Option<PathBuf>,
}

pub struct App {
    pub host: Host,
    pub agent: Option<Agent>,
    pub generation: u64,
    pub model: ModelSelection,
    pub thinking: Thinking,
    pub tier: ServiceTier,
    pub transcript: Transcript,
    pub mapper: EventMapper,
    pub composer: Composer,
    pub queue: Queue,
    pub turn: TurnState,
    pub overlay: Option<Overlay>,
    pub tree: AgentTree,
    pub show_tree: bool,
    pub theme: Theme,
    pub status: Option<(String, Instant)>,
    pub starting: bool,
    pub remote_label: Option<String>,
    pub pending_switch: Option<(ModelSelection, Thinking, ServiceTier)>,
    pub spinner: Spinner,
    pub(crate) pending_pro: Option<bool>,
    toggle_mouse: bool,
    awaiting_terminal: bool,
    pub(crate) session_started: bool,
    pub(crate) tx: UnboundedSender<AppEvent>,
    quit: bool,
    quit_armed: Option<Instant>,
    esc_armed: Option<Instant>,
    stashed_draft: Option<String>,
    pub(crate) btw: HashMap<u64, usize>,
    pub(crate) next_btw: u64,
    next_local: u64,
    history_path: Option<PathBuf>,
}

pub async fn run(launch: Launch, mut terminal: TerminalSession) -> Result<()> {
    let (tx, rx) = unbounded_channel();
    let history = launch
        .history_path
        .as_ref()
        .map(|path| crate::history::load(path))
        .unwrap_or_default();
    let mut app = App {
        host: launch.host.clone(),
        agent: None,
        generation: 0,
        model: launch.model.clone(),
        thinking: launch.thinking,
        tier: launch.tier,
        transcript: Transcript::new(TranscriptPolicy {
            collapsed_preview_lines: TOOL_PREVIEW_LINES,
            collapsed_diff_lines: DIFF_PREVIEW_LINES,
        }),
        mapper: EventMapper::default(),
        composer: Composer::new("Ask anything · / for commands", History::new(history)),
        queue: Queue::default(),
        turn: TurnState::Idle,
        overlay: None,
        tree: AgentTree::default(),
        show_tree: true,
        theme: Theme::default(),
        status: None,
        starting: true,
        remote_label: launch.remote_label.clone(),
        pending_switch: None,
        spinner: Spinner::new(Instant::now(), SPINNER_FRAME),
        pending_pro: None,
        toggle_mouse: false,
        awaiting_terminal: false,
        session_started: false,
        tx,
        quit: false,
        quit_armed: None,
        esc_armed: None,
        stashed_draft: None,
        btw: HashMap::new(),
        next_btw: 0,
        next_local: 0,
        history_path: launch.history_path.clone(),
    };
    if let Some(updates) = app.host.runtime.take_subagent_updates() {
        forward_subagents(updates, app.tx.clone());
    }
    if let Some(prompt) = launch
        .prompt
        .clone()
        .filter(|prompt| !prompt.trim().is_empty())
    {
        app.enqueue_follow_up(prompt);
    }
    if launch.pick_session {
        app.starting = false;
        app.open_session_picker();
    } else {
        app.connect(launch.resume.clone());
    }
    let outcome = app.event_loop(&mut terminal, rx).await;
    drop(terminal);
    if let Some(agent) = app.agent.take() {
        drop(agent.flush().await);
    }
    drop(app.host.runtime.shutdown().await);
    if let Some(agent_session) = app.session_id() {
        eprintln!(
            "nanocodex: session {agent_session}  (resume with: nanocodex --resume {agent_session})"
        );
    }
    outcome
}

fn forward_subagents(mut updates: nanocode::SubagentUpdates, tx: UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        while let Some(update) = updates.recv().await {
            if tx.send(AppEvent::Subagent(update)).is_err() {
                break;
            }
        }
    });
}

impl App {
    pub fn session_id(&self) -> Option<String> {
        self.agent
            .as_ref()
            .map(|agent| agent.session_id().to_owned())
    }

    async fn event_loop(
        &mut self,
        terminal: &mut TerminalSession,
        mut rx: UnboundedReceiver<AppEvent>,
    ) -> Result<()> {
        let mut input = EventStream::new();
        let mut scheduler = RenderScheduler::new(DISPLAY_FRAME, Instant::now());
        while !self.quit {
            let now = Instant::now();
            if scheduler.is_due(now) {
                terminal.draw(|frame| view::draw(self, frame))?;
                scheduler.presented(Instant::now());
                if self.animating() {
                    scheduler.request_at(self.spinner.next_deadline(Instant::now()));
                }
                if let Some((_, at)) = &self.status {
                    let expiry = *at + view::STATUS_VISIBLE;
                    if expiry > Instant::now() {
                        scheduler.request_at(expiry);
                    }
                }
            }
            let deadline = scheduler.deadline().map(tokio::time::Instant::from_std);
            tokio::select! {
                event = input.next() => {
                    match event {
                        Some(Ok(event)) => {
                            if self.handle_terminal(event, terminal) {
                                scheduler.request_immediate(Instant::now());
                            }
                        }
                        Some(Err(error)) => return Err(error.into()),
                        None => break,
                    }
                }
                event = rx.recv() => {
                    let Some(event) = event else { break };
                    let immediate = self.handle_app(event);
                    while let Ok(event) = rx.try_recv() {
                        self.handle_app(event);
                    }
                    if immediate {
                        scheduler.request_immediate(Instant::now());
                    } else {
                        scheduler.request_streaming(Instant::now());
                    }
                }
                () = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                } => {}
            }
        }
        Ok(())
    }

    fn animating(&self) -> bool {
        !matches!(self.turn, TurnState::Idle)
            || self.starting
            || !self.btw.is_empty()
            || self.tree.active() > 0
    }

    pub fn notice(&mut self, level: NoticeLevel, text: impl Into<String>) {
        self.transcript.push(CellKind::Notice(level, text.into()));
    }

    pub fn flash(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), Instant::now()));
    }

    fn local_id(&mut self, prefix: &str) -> String {
        self.next_local += 1;
        format!("{prefix}-{}-{}", std::process::id(), self.next_local)
    }

    pub fn connect(&mut self, resume: Option<String>) {
        self.starting = true;
        let host = self.host.clone();
        let model = self.model.clone();
        let thinking = self.thinking;
        let tier = self.tier;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let ready = async {
                match resume {
                    Some(session_id) => open_session(&host, &session_id, tier).await,
                    None => {
                        let (agent, events) = host.start(&model, thinking).await?;
                        apply_tier(&agent, &model, tier).await?;
                        Ok(Ready {
                            notice: format!("new session {}", agent.session_id()),
                            agent,
                            events,
                            model,
                            thinking,
                            tier,
                            host: None,
                            history: None,
                            reset_transcript: false,
                        })
                    }
                }
            };
            let event = match ready.await {
                Ok(ready) => AppEvent::AgentReady(Box::new(ready)),
                Err(error) => AppEvent::AgentFailed(error.to_string()),
            };
            drop(tx.send(event));
        });
    }

    fn handle_app(&mut self, event: AppEvent) -> bool {
        match event {
            AppEvent::Agent { generation, event } => {
                if generation == self.generation {
                    self.agent_event(&event);
                }
                false
            }
            AppEvent::TurnStarted {
                generation,
                control,
            } => {
                if generation != self.generation {
                    return false;
                }
                let since = match self.turn {
                    TurnState::Starting(since) => since,
                    _ => Instant::now(),
                };
                self.turn = TurnState::Running { control, since };
                self.flush_pending_steers();
                true
            }
            AppEvent::RoutedTurn {
                generation,
                id,
                control,
            } => {
                if generation != self.generation {
                    return false;
                }
                self.queue.convert(&id, QueueKind::FollowUp);
                self.queue.set_state(&id, QueueState::Sent);
                self.mapper.begin_turn();
                self.turn = TurnState::Running {
                    control,
                    since: Instant::now(),
                };
                self.flush_pending_steers();
                true
            }
            AppEvent::TurnFinished { generation, result } => {
                if generation != self.generation {
                    return false;
                }
                self.turn = TurnState::Idle;
                match result {
                    Err(error) if !self.mapper.reported() => {
                        self.transcript.finish_running_tools(ToolStatus::Failed);
                        self.notice(NoticeLevel::Error, error);
                        self.settle_turn();
                    }
                    _ if self.mapper.reported() => self.settle_turn(),
                    _ => self.awaiting_terminal = true,
                }
                true
            }
            AppEvent::PromptFailed {
                generation,
                error,
                text,
            } => {
                if generation != self.generation {
                    return false;
                }
                self.turn = TurnState::Idle;
                let steers = self.queue.drain_kind(QueueKind::Steer);
                if let Some(item) = self
                    .queue
                    .items()
                    .iter()
                    .find(|item| item.state == QueueState::Sent)
                    .map(|item| item.id.clone())
                {
                    self.queue.remove(&item);
                }
                self.notice(NoticeLevel::Error, format!("prompt failed: {error}"));
                let mut restored = vec![text];
                restored.extend(steers.into_iter().map(|steer| steer.text));
                if !self.composer.is_empty() {
                    restored.push(self.composer.text().to_owned());
                }
                self.composer.set_text(restored.join("\n\n"));
                true
            }
            AppEvent::SteerSent { id, result } => {
                match result {
                    Ok(()) => {
                        self.queue.set_state(&id, QueueState::Sent);
                    }
                    Err(error) => {
                        if self.queue.convert(&id, QueueKind::FollowUp) {
                            self.flash(format!(
                                "steer not admitted ({error}); queued as follow-up"
                            ));
                        }
                        if matches!(self.turn, TurnState::Idle) {
                            self.after_turn();
                        }
                    }
                }
                true
            }
            AppEvent::AgentReady(ready) => {
                self.ready(*ready);
                true
            }
            AppEvent::AgentFailed(error) => {
                self.starting = false;
                self.notice(
                    NoticeLevel::Error,
                    format!("failed to start agent: {error}"),
                );
                true
            }
            AppEvent::Sessions(result) => {
                match result {
                    Ok(sessions) if sessions.is_empty() => {
                        self.flash("no saved sessions for this workspace");
                        if self.agent.is_none() && !self.starting {
                            self.connect(None);
                        }
                    }
                    Ok(sessions) => {
                        let picker = crate::sessions::picker(&sessions);
                        self.overlay =
                            Some(Overlay::Picker(PickerPurpose::Session(sessions), picker));
                    }
                    Err(error) => {
                        self.notice(
                            NoticeLevel::Error,
                            format!("failed to list sessions: {error}"),
                        );
                        if self.agent.is_none() && !self.starting {
                            self.connect(None);
                        }
                    }
                }
                true
            }
            AppEvent::Btw { id, update } => {
                self.btw_update(id, update);
                false
            }
            AppEvent::Subagent(update) => {
                self.subagent(update);
                false
            }
            AppEvent::Notice(level, text) => {
                self.notice(level, text);
                true
            }
            AppEvent::Applied(result) => {
                self.starting = false;
                match result {
                    Ok((model, thinking, tier, notice)) => {
                        self.model = model;
                        self.thinking = thinking;
                        self.tier = tier;
                        self.notice(NoticeLevel::Info, notice);
                    }
                    Err(error) => {
                        self.notice(NoticeLevel::Error, format!("failed to switch: {error}"))
                    }
                }
                self.after_turn();
                true
            }
        }
    }

    fn ready(&mut self, ready: Ready) {
        self.starting = false;
        self.generation += 1;
        if let Some(host) = ready.host {
            self.host = host;
            if let Some(updates) = self.host.runtime.take_subagent_updates() {
                forward_subagents(updates, self.tx.clone());
            }
        }
        if ready.reset_transcript {
            self.transcript.clear();
            self.mapper.reset();
            self.tree = AgentTree::default();
        }
        self.session_started = ready
            .history
            .as_ref()
            .is_some_and(|history| !history.is_empty());
        if let Some(history) = ready.history {
            crate::sessions::replay(&mut self.transcript, history);
        }
        self.model = ready.model;
        self.thinking = ready.thinking;
        self.tier = ready.tier;
        self.turn = TurnState::Idle;
        self.awaiting_terminal = false;
        let generation = self.generation;
        let tx = self.tx.clone();
        let mut events = ready.events;
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if tx.send(AppEvent::Agent { generation, event }).is_err() {
                    break;
                }
            }
        });
        self.agent = Some(ready.agent);
        self.notice(NoticeLevel::Info, ready.notice);
        self.after_turn();
    }

    fn agent_event(&mut self, event: &AgentEvent) {
        let mapped = self.mapper.apply(&mut self.transcript, event);
        if event.kind == AgentEventKind::RunStarted
            && matches!(self.turn, TurnState::Idle)
            && !self.starting
        {
            self.turn = TurnState::External(Instant::now());
        }
        if event.kind.is_terminal() && matches!(self.turn, TurnState::External(_)) {
            self.turn = TurnState::Idle;
            self.settle_turn();
        }
        if mapped.accepted.is_some() {
            self.session_started = true;
        }
        if event.kind.is_terminal()
            && self.awaiting_terminal
            && matches!(self.turn, TurnState::Idle)
        {
            self.settle_turn();
        }
        match mapped.accepted {
            Some(Accepted::Steer { id, text }) => {
                let removed = id.and_then(|id| self.queue.remove(&id));
                if removed.is_none() {
                    let found = self
                        .queue
                        .items()
                        .iter()
                        .find(|item| item.kind == QueueKind::Steer && item.text == text)
                        .map(|item| item.id.clone());
                    if let Some(found) = found {
                        self.queue.remove(&found);
                    }
                }
            }
            Some(Accepted::Prompt { request_id }) => {
                let found = request_id
                    .and_then(|request| self.queue.remove(&request))
                    .or_else(|| {
                        let sent = self
                            .queue
                            .items()
                            .iter()
                            .find(|item| {
                                item.kind == QueueKind::FollowUp && item.state == QueueState::Sent
                            })
                            .map(|item| item.id.clone())?;
                        self.queue.remove(&sent)
                    });
                drop(found);
            }
            None => {}
        }
    }

    fn settle_turn(&mut self) {
        self.awaiting_terminal = false;
        let unaccepted = self
            .queue
            .items()
            .iter()
            .filter(|item| item.kind == QueueKind::Steer && item.state == QueueState::Sent)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        for id in unaccepted {
            self.queue.convert(&id, QueueKind::FollowUp);
        }
        self.after_turn();
    }

    fn after_turn(&mut self) {
        if !matches!(self.turn, TurnState::Idle) || self.agent.is_none() {
            return;
        }
        if let Some((model, thinking, tier)) = self.pending_switch.take() {
            self.switch(model, thinking, tier);
            return;
        }
        if self
            .queue
            .items()
            .iter()
            .any(|item| item.state == QueueState::Sent && item.kind == QueueKind::FollowUp)
        {
            return;
        }
        let next = self
            .queue
            .items()
            .iter()
            .find(|item| item.kind == QueueKind::FollowUp || item.state == QueueState::Pending)
            .map(|item| item.id.clone());
        let Some(id) = next else { return };
        let Some(item) = self.queue.remove(&id) else {
            return;
        };
        let mut text = item.text;
        let extra = self.queue.drain_kind(QueueKind::Steer);
        for steer in extra {
            text.push_str("\n\n");
            text.push_str(&steer.text);
        }
        self.start_turn(text);
    }

    pub(crate) fn start_turn(&mut self, text: String) {
        let Some(agent) = self.agent.clone() else {
            self.enqueue_follow_up(text);
            return;
        };
        let id = uuid::Uuid::now_v7().to_string();
        self.queue.push(QueuedPrompt {
            id: id.clone(),
            kind: QueueKind::FollowUp,
            state: QueueState::Sent,
            text: text.clone(),
        });
        self.turn = TurnState::Starting(Instant::now());
        self.mapper.begin_turn();
        self.transcript.scroll_to_bottom();
        let generation = self.generation;
        let tx = self.tx.clone();
        let sent = text.clone();
        tokio::spawn(async move {
            match agent
                .prompt(PromptRequest::new(sent.as_str()).request_id(id))
                .await
            {
                Ok(turn) => {
                    drop(tx.send(AppEvent::TurnStarted {
                        generation,
                        control: turn.control(),
                    }));
                    let result = turn.await.map_err(|error| error.to_string());
                    drop(tx.send(AppEvent::TurnFinished { generation, result }));
                }
                Err(error) => {
                    drop(tx.send(AppEvent::PromptFailed {
                        generation,
                        error: error.to_string(),
                        text,
                    }));
                }
            }
        });
    }

    fn enqueue_follow_up(&mut self, text: String) {
        let id = self.local_id("follow");
        self.queue.push(QueuedPrompt {
            id,
            kind: QueueKind::FollowUp,
            state: QueueState::Pending,
            text,
        });
    }

    fn flush_pending_steers(&mut self) {
        if matches!(self.turn, TurnState::External(_)) {
            self.route_pending_steers();
            return;
        }
        let TurnState::Running { control, .. } = &self.turn else {
            return;
        };
        let pending = self
            .queue
            .items()
            .iter()
            .filter(|item| item.kind == QueueKind::Steer && item.state == QueueState::Pending)
            .map(|item| (item.id.clone(), item.text.clone()))
            .collect::<Vec<_>>();
        for (id, text) in pending {
            self.queue.set_state(&id, QueueState::Sent);
            let control = control.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = control
                    .steer_with_id(id.clone(), text.as_str())
                    .await
                    .map_err(|error| error.to_string());
                drop(tx.send(AppEvent::SteerSent { id, result }));
            });
        }
    }

    fn route_pending_steers(&mut self) {
        let Some(agent) = self.agent.clone() else {
            return;
        };
        let pending = self
            .queue
            .items()
            .iter()
            .filter(|item| item.kind == QueueKind::Steer && item.state == QueueState::Pending)
            .map(|item| (item.id.clone(), item.text.clone()))
            .collect::<Vec<_>>();
        let generation = self.generation;
        for (id, text) in pending {
            self.queue.set_state(&id, QueueState::Sent);
            let agent = agent.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match agent.route_prompt(text.as_str()).await {
                    Ok(PromptRoute::Steered) => {
                        drop(tx.send(AppEvent::SteerSent { id, result: Ok(()) }))
                    }
                    Ok(PromptRoute::Started(turn)) => {
                        drop(tx.send(AppEvent::RoutedTurn {
                            generation,
                            id,
                            control: turn.control(),
                        }));
                        let result = turn.await.map_err(|error| error.to_string());
                        drop(tx.send(AppEvent::TurnFinished { generation, result }));
                    }
                    Err(error) => drop(tx.send(AppEvent::SteerSent {
                        id,
                        result: Err(error.to_string()),
                    })),
                }
            });
        }
    }

    fn interrupt(&mut self) {
        let TurnState::Running { control, .. } = &self.turn else {
            match self.turn {
                TurnState::Starting(_) => self.flash("turn is starting; press esc again once it runs"),
                TurnState::External(_) => self.flash(
                    "this turn was started by the SDK after a subagent finished; it can't be interrupted from here",
                ),
                _ => {}
            }
            return;
        };
        let control = control.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(error) = control.cancel().await {
                drop(tx.send(AppEvent::Notice(
                    NoticeLevel::Error,
                    format!("interrupt failed: {error}"),
                )));
            }
        });
        let restored = self
            .queue
            .items()
            .iter()
            .filter(|item| !(item.kind == QueueKind::FollowUp && item.state == QueueState::Sent))
            .map(|item| item.text.clone())
            .collect::<Vec<_>>();
        let ids = self
            .queue
            .items()
            .iter()
            .filter(|item| !(item.kind == QueueKind::FollowUp && item.state == QueueState::Sent))
            .map(|item| (item.id.clone(), item.kind, item.state))
            .collect::<Vec<_>>();
        for (id, kind, state) in ids {
            if kind == QueueKind::Steer && state == QueueState::Sent {
                let control = control_of(&self.turn);
                if let Some(control) = control {
                    let id = id.clone();
                    tokio::spawn(async move {
                        drop(control.withdraw_steer(id).await);
                    });
                }
            }
            self.queue.remove(&id);
        }
        if !restored.is_empty() {
            let mut text = restored.join("\n\n");
            if !self.composer.is_empty() {
                text = format!("{text}\n\n{}", self.composer.text());
            }
            self.composer.set_text(text);
        }
        self.flash("interrupting…");
    }

    fn handle_terminal(&mut self, event: Event, terminal: &mut TerminalSession) -> bool {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.handle_key(key, terminal);
                if std::mem::take(&mut self.toggle_mouse) {
                    let enabled = !terminal.mouse_capture();
                    drop(terminal.set_mouse_capture(enabled));
                    self.flash(if enabled {
                        "mouse scrolling on"
                    } else {
                        "mouse scrolling off (native selection)"
                    });
                }
                true
            }
            Event::Paste(text) => {
                if let Some(Overlay::Picker(_, picker)) = &mut self.overlay {
                    for character in text.chars().filter(|character| !character.is_control()) {
                        picker.handle_key(KeyEvent::new(
                            KeyCode::Char(character),
                            KeyModifiers::NONE,
                        ));
                    }
                } else {
                    self.composer.paste(&text);
                }
                true
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.transcript.scroll_up(3);
                    true
                }
                MouseEventKind::ScrollDown => {
                    self.transcript.scroll_down(3);
                    true
                }
                _ => false,
            },
            Event::Resize(..) | Event::FocusGained => true,
            _ => false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent, terminal: &mut TerminalSession) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if self.overlay.is_some() {
            self.overlay_key(key);
            return;
        }
        let busy = !matches!(self.turn, TurnState::Idle);
        if key.code != KeyCode::Esc {
            self.esc_armed = None;
        }
        match key.code {
            KeyCode::Char('c') if control => {
                if !self.composer.is_empty() {
                    self.stashed_draft = Some(self.composer.take());
                    self.flash("draft cleared · ctrl+z to restore");
                } else if self
                    .quit_armed
                    .is_some_and(|armed| armed.elapsed() < QUIT_CONFIRMATION)
                {
                    self.quit = true;
                } else {
                    self.quit_armed = Some(Instant::now());
                    self.flash(if busy {
                        "press ctrl+c again to quit · esc esc interrupts the turn"
                    } else {
                        "press ctrl+c again to quit"
                    });
                }
            }
            KeyCode::Char('z') if control => match self.stashed_draft.take() {
                Some(draft) => {
                    let current = self.composer.take();
                    self.composer.set_text(if current.is_empty() {
                        draft
                    } else {
                        format!("{draft}\n{current}")
                    });
                }
                None => self.flash("no cleared draft to restore"),
            },
            KeyCode::Esc => {
                if busy {
                    if self
                        .esc_armed
                        .is_some_and(|armed| armed.elapsed() < QUIT_CONFIRMATION)
                    {
                        self.esc_armed = None;
                        self.interrupt();
                    } else {
                        self.esc_armed = Some(Instant::now());
                        self.flash("press esc again to interrupt");
                    }
                } else if self.queue.selected().is_some() {
                    self.queue.clear_selection();
                }
            }
            KeyCode::Char('d') if control => self.command("model"),
            KeyCode::Char('s') if control => self.command("thinking"),
            KeyCode::Char('o') if control => self.transcript.toggle_expanded(),
            KeyCode::Char('t') if control => self.show_tree = !self.show_tree,
            KeyCode::Char('r') if control => self.open_recent_prompts(),
            KeyCode::Char('g') if control => self.edit_externally(terminal),
            KeyCode::Char('l') if control => drop(terminal.clear()),
            KeyCode::F(1) => self.overlay = Some(Overlay::Help),
            KeyCode::PageUp => self.transcript.scroll_up(self.transcript.page()),
            KeyCode::PageDown => self.transcript.scroll_down(self.transcript.page()),
            KeyCode::Home if control => self.transcript.scroll_to_top(),
            KeyCode::End if control => self.transcript.scroll_to_bottom(),
            KeyCode::Up if alt => self.recall_queued(),
            KeyCode::Char('u') if alt => self.recall_queued(),
            KeyCode::Tab
                if self.composer.text().starts_with('/') && !self.composer.text().contains(' ') =>
            {
                if let Some(command) = crate::commands::complete(self.composer.text()) {
                    self.composer.set_text(format!("{command} "));
                }
            }
            KeyCode::Tab if !self.composer.is_empty() && busy => {
                let text = self.composer.take();
                self.composer.history_mut().push(text.clone());
                self.append_history(&text);
                self.enqueue_follow_up(text);
            }
            KeyCode::Enter if busy && self.composer.is_empty() && key.modifiers.is_empty() => {
                self.promote_follow_up();
            }
            _ => {
                if let ComposerAction::Submit(text) = self.composer.handle_key(key) {
                    self.quit_armed = None;
                    self.submit(text);
                }
            }
        }
    }

    fn promote_follow_up(&mut self) {
        let latest = self
            .queue
            .items()
            .iter()
            .rev()
            .find(|item| item.kind == QueueKind::FollowUp && item.state == QueueState::Pending)
            .map(|item| item.id.clone());
        match latest {
            Some(id) => {
                self.queue.convert(&id, QueueKind::Steer);
                self.flush_pending_steers();
            }
            None => self.flash("nothing queued to steer with"),
        }
    }

    fn open_recent_prompts(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let items = self
            .composer
            .history()
            .entries()
            .iter()
            .rev()
            .filter(|entry| !entry.starts_with('/') && seen.insert(entry.as_str()))
            .map(|entry| PickerItem::choice(entry.replace('\n', " ⏎ ")))
            .collect::<Vec<_>>();
        if items.is_empty() {
            self.flash("no recent prompts");
            return;
        }
        let entries = self
            .composer
            .history()
            .entries()
            .iter()
            .rev()
            .filter(|entry| !entry.starts_with('/'))
            .fold(Vec::<String>::new(), |mut unique, entry| {
                if !unique.contains(entry) {
                    unique.push(entry.clone());
                }
                unique
            });
        let picker = Picker::new("Recent prompts", items)
            .filterable()
            .footer("type to filter · enter to edit · esc cancel");
        self.overlay = Some(Overlay::Picker(PickerPurpose::Recent(entries), picker));
    }

    fn edit_externally(&mut self, terminal: &mut TerminalSession) {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_owned());
        let path = std::env::temp_dir().join(format!("nanocodex-prompt-{}.md", std::process::id()));
        if let Err(error) = std::fs::write(&path, self.composer.text()) {
            self.notice(
                NoticeLevel::Error,
                format!("could not write {}: {error}", path.display()),
            );
            return;
        }
        let Some(words) = shlex_words(&editor) else {
            self.notice(
                NoticeLevel::Error,
                format!("could not parse editor command {editor:?}"),
            );
            return;
        };
        drop(terminal.suspend());
        let status = std::process::Command::new(&words[0])
            .args(&words[1..])
            .arg(&path)
            .status();
        drop(terminal.resume());
        match status {
            Ok(status) if status.success() => match std::fs::read_to_string(&path) {
                Ok(text) => self.composer.set_text(text.trim_end_matches('\n')),
                Err(error) => self.notice(
                    NoticeLevel::Error,
                    format!("could not read {}: {error}", path.display()),
                ),
            },
            Ok(status) => self.flash(format!("{editor} exited with {status}; draft unchanged")),
            Err(error) => self.notice(
                NoticeLevel::Error,
                format!("could not run {editor}: {error}"),
            ),
        }
        drop(std::fs::remove_file(&path));
    }

    fn recall_queued(&mut self) {
        let Some(item) = self.queue.items().last().cloned() else {
            self.flash("nothing queued");
            return;
        };
        if item.kind == QueueKind::FollowUp && item.state == QueueState::Sent {
            self.flash("prompt already sent");
            return;
        }
        if item.kind == QueueKind::Steer
            && item.state == QueueState::Sent
            && let Some(control) = control_of(&self.turn)
        {
            let id = item.id.clone();
            let tx = self.tx.clone();
            let text = item.text.clone();
            tokio::spawn(async move {
                match control.withdraw_steer(id).await {
                    Ok(true) => {}
                    Ok(false) => drop(tx.send(AppEvent::Notice(
                        NoticeLevel::Warning,
                        format!("steer was already delivered: {text}"),
                    ))),
                    Err(error) => drop(tx.send(AppEvent::Notice(
                        NoticeLevel::Warning,
                        format!("withdraw failed: {error}"),
                    ))),
                }
            });
        }
        self.queue.remove(&item.id);
        let text = if self.composer.is_empty() {
            item.text
        } else {
            format!("{}\n{}", item.text, self.composer.text())
        };
        self.composer.set_text(text);
    }

    fn submit(&mut self, text: String) {
        self.transcript.scroll_to_bottom();
        self.append_history(&text);
        self.stashed_draft = None;
        if let Some(command) = text.strip_prefix('/')
            && !command.starts_with('/')
        {
            self.command(command.trim());
            return;
        }
        match self.turn {
            TurnState::Running { .. } | TurnState::Starting(_) | TurnState::External(_) => {
                self.enqueue_follow_up(text);
                self.flash("queued · enter on an empty prompt steers it into the running turn");
            }
            TurnState::Idle
                if self.agent.is_none() || self.pending_switch.is_some() || self.starting =>
            {
                self.enqueue_follow_up(text);
            }
            TurnState::Idle => self.start_turn(text),
        }
    }

    fn append_history(&self, text: &str) {
        if let Some(path) = &self.history_path {
            crate::history::append(path, text);
        }
    }

    fn command(&mut self, command: &str) {
        let (name, argument) = command
            .split_once(' ')
            .map_or((command, ""), |(name, rest)| (name, rest.trim()));
        match name {
            "model" | "m" if argument.is_empty() => {
                self.overlay = Some(Overlay::Picker(
                    PickerPurpose::Model,
                    models::model_picker(&self.model),
                ));
            }
            "model" | "m" => match crate::host::parse_model(argument) {
                Ok(model) => {
                    let picker = models::thinking_picker(&model, self.thinking);
                    self.overlay = Some(Overlay::Picker(PickerPurpose::Thinking(model), picker));
                }
                Err(error) => self.notice(NoticeLevel::Error, error.to_string()),
            },
            "thinking" | "effort" | "reasoning" if argument.is_empty() => {
                let model = self.model.clone();
                let picker = models::thinking_picker(&model, self.thinking);
                self.overlay = Some(Overlay::Picker(PickerPurpose::Thinking(model), picker));
            }
            "thinking" | "effort" | "reasoning" => match crate::host::parse_thinking(argument) {
                Ok(thinking) => self.request_switch(self.model.clone(), thinking, self.tier),
                Err(error) => self.notice(NoticeLevel::Error, error.to_string()),
            },
            "fast" | "tier" => {
                let capabilities = self.model.capabilities(nanocode::ModelTransport::Native);
                let tiers = capabilities.service_tiers().collect::<Vec<_>>();
                let tier = if argument.is_empty() {
                    let current = tiers
                        .iter()
                        .position(|tier| *tier == self.tier)
                        .unwrap_or(0);
                    tiers[(current + 1) % tiers.len()]
                } else {
                    match crate::host::parse_tier(argument) {
                        Ok(tier) => tier,
                        Err(error) => {
                            self.notice(NoticeLevel::Error, error.to_string());
                            return;
                        }
                    }
                };
                self.request_switch(self.model.clone(), self.thinking, tier);
            }
            "btw" if argument.is_empty() => self.flash("usage: /btw <question>"),
            "btw" => self.btw(argument.to_owned()),
            "resume" | "sessions" => self.open_session_picker(),
            "new" | "clear" => self.new_session(),
            "agents" => self.show_tree = !self.show_tree,
            "compact" => self.compact(),
            "session" | "status" => {
                let session = self.session_id().unwrap_or_else(|| "not started".into());
                let store = if self.host.config.store.is_some() {
                    "durable"
                } else {
                    "ephemeral"
                };
                let remote = self
                    .remote_label
                    .clone()
                    .unwrap_or_else(|| "local tools".into());
                self.notice(
                    NoticeLevel::Info,
                    format!(
                        "session {session} ({store}) · {} · thinking {} · {} · {} · {remote}",
                        self.model,
                        self.thinking,
                        self.tier.as_str(),
                        self.host.config.workspace.display()
                    ),
                );
            }
            "expand" | "collapse" => self.transcript.toggle_expanded(),
            "mouse" => self.toggle_mouse = true,
            "help" | "keys" | "?" => self.overlay = Some(Overlay::Help),
            "quit" | "exit" | "q" => self.quit = true,
            other => self.notice(
                NoticeLevel::Warning,
                format!("unknown command /{other}; try /help"),
            ),
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) {
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        let action = match overlay {
            Overlay::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::F(1)
                ) {
                    self.overlay = None;
                }
                return;
            }
            Overlay::Picker(_, picker) => picker.handle_key(key),
        };
        match action {
            PickerAction::Cancelled => {
                let Some(Overlay::Picker(purpose, _)) = self.overlay.take() else {
                    return;
                };
                if matches!(purpose, PickerPurpose::Session(_))
                    && self.agent.is_none()
                    && !self.starting
                {
                    self.connect(None);
                }
            }
            PickerAction::Selected(index) => {
                let Some(Overlay::Picker(purpose, picker)) = self.overlay.take() else {
                    return;
                };
                self.picked(purpose, &picker, index);
            }
            PickerAction::Changed | PickerAction::Ignored => {}
        }
    }

    fn picked(&mut self, purpose: PickerPurpose, picker: &Picker, index: usize) {
        match purpose {
            PickerPurpose::Model => {
                let Some(model) = models::model_at(picker, index) else {
                    return;
                };
                let thinking = if model.supports_thinking(self.thinking) {
                    self.thinking
                } else {
                    model.default_thinking()
                };
                let picker = models::thinking_picker(&model, thinking);
                self.overlay = Some(Overlay::Picker(PickerPurpose::Thinking(model), picker));
            }
            PickerPurpose::Thinking(model) => {
                let Some(thinking) = models::thinking_at(&model, picker, index) else {
                    return;
                };
                let tiers = models::tier_options(&model);
                if tiers.len() > 1 {
                    let picker = models::tier_picker(&model, &tiers, self.tier, self.host.pro);
                    self.overlay = Some(Overlay::Picker(
                        PickerPurpose::Tier(model, thinking),
                        picker,
                    ));
                } else {
                    self.request_switch(model, thinking, ServiceTier::Standard);
                }
            }
            PickerPurpose::Tier(model, thinking) => {
                let tiers = models::tier_options(&model);
                let Some(option) = tiers.get(index).copied() else {
                    return;
                };
                match option {
                    models::TierOption::Tier(tier) => {
                        if self.host.pro && model.as_codex().is_some() {
                            self.pending_pro = Some(false);
                        }
                        self.request_switch(model, thinking, tier);
                    }
                    models::TierOption::Pro => {
                        self.pending_pro = Some(true);
                        self.request_switch(model, thinking, ServiceTier::Standard);
                    }
                }
            }
            PickerPurpose::Recent(entries) => {
                if let Some(entry) = entries.get(index) {
                    self.composer.set_text(entry.clone());
                }
            }
            PickerPurpose::Session(sessions) => {
                let Some(session) = crate::sessions::session_at(&sessions, picker, index) else {
                    return;
                };
                self.resume(session.record.session_id.clone());
            }
        }
    }
}

fn control_of(turn: &TurnState) -> Option<TurnControl> {
    match turn {
        TurnState::Running { control, .. } => Some(control.clone()),
        _ => None,
    }
}

pub async fn apply_tier(agent: &Agent, model: &ModelSelection, tier: ServiceTier) -> Result<()> {
    if tier != ServiceTier::Standard || model.supports_fast_mode() {
        agent.set_service_tier(tier).await?;
    }
    Ok(())
}

pub async fn open_session(host: &Host, session_id: &str, tier: ServiceTier) -> Result<Ready> {
    let store = host
        .config
        .store
        .as_ref()
        .ok_or("resume requires durability; remove --ephemeral")?;
    let stored = store.load(session_id).await?;
    let model = stored.summary.record.model.clone();
    let thinking = stored.session_checkpoint()?.map_or_else(
        || model.default_thinking(),
        |checkpoint| checkpoint.thinking(),
    );
    let (agent, events) = host.open(session_id).await?;
    let tier = model
        .capabilities(nanocode::ModelTransport::Native)
        .normalize_service_tier(tier);
    apply_tier(&agent, &model, tier).await?;
    Ok(Ready {
        notice: format!("resumed session {session_id} · {model}"),
        agent,
        events,
        model,
        thinking,
        tier,
        host: None,
        history: Some(stored.transcript),
        reset_transcript: true,
    })
}

pub fn subagent_status(status: &AgentStatus) -> (NodeStatus, String) {
    match status {
        AgentStatus::Pending => (NodeStatus::Pending, String::new()),
        AgentStatus::Running => (NodeStatus::Running, String::new()),
        AgentStatus::Completed { output } => {
            let text = match output {
                serde_json::Value::String(text) => text.clone(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            };
            (
                NodeStatus::Completed,
                text.lines().next().unwrap_or_default().to_owned(),
            )
        }
        AgentStatus::Interrupted => (NodeStatus::Idle, "interrupted".into()),
        AgentStatus::Failed { error } => (NodeStatus::Failed, error.clone()),
        AgentStatus::Closing | AgentStatus::Closed => (NodeStatus::Closed, String::new()),
    }
}

pub fn family_label(family: &HarnessFamily) -> &'static str {
    if *family == HarnessFamily::Claude {
        "Claude"
    } else {
        "Codex"
    }
}

fn shlex_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    for character in command.chars() {
        match (quote, character) {
            (Some(open), character) if character == open => quote = None,
            (Some(_), character) => current.push(character),
            (None, '"' | '\'') => {
                quote = Some(character);
                started = true;
            }
            (None, character) if character.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, character) => {
                current.push(character);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}
