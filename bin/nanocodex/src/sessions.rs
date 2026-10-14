use std::time::{SystemTime, UNIX_EPOCH};

use nanocode::durability::{SessionSummary, TranscriptItem};
use nanocode_tui::{
    picker::{Picker, PickerItem},
    transcript::{CellKind, ToolBody, ToolCell, ToolStatus, Transcript},
};

pub fn age(updated_at_ms: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        });
    let seconds = now.saturating_sub(updated_at_ms) / 1000;
    match seconds {
        0..60 => format!("{seconds}s ago"),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86_400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

pub fn title(session: &SessionSummary) -> String {
    session
        .record
        .title
        .clone()
        .or_else(|| session.preview.clone())
        .map(|text| {
            let text = text
                .rsplit_once(crate::events::HANDOFF_END)
                .map_or(text.as_str(), |(_, message)| message);
            text.lines().next().unwrap_or_default().to_owned()
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "(no prompt yet)".into())
}

pub fn picker(sessions: &[SessionSummary]) -> Picker {
    let items = sessions
        .iter()
        .map(|session| {
            PickerItem::choice(title(session)).detail(format!(
                "{} · {} · {}",
                age(session.record.updated_at_ms),
                session.record.model,
                session.record.session_id
            ))
        })
        .collect();
    Picker::new("Resume session", items)
        .filterable()
        .footer("type to filter · enter resume · esc cancel")
}

pub fn session_at<'a>(
    sessions: &'a [SessionSummary],
    picker: &Picker,
    index: usize,
) -> Option<&'a SessionSummary> {
    let item = picker.items().get(index)?;
    sessions
        .iter()
        .find(|session| item.detail.ends_with(&session.record.session_id))
}

pub fn replay(transcript: &mut Transcript, history: Vec<TranscriptItem>) {
    let mut tools = std::collections::HashMap::new();
    for item in history {
        match item {
            TranscriptItem::User(text) => {
                transcript.push(CellKind::User(text));
            }
            TranscriptItem::Assistant(text) => {
                transcript.push(CellKind::Assistant(text));
            }
            TranscriptItem::Reasoning(text) => {
                if !text.trim().is_empty() {
                    transcript.push(CellKind::Reasoning(text));
                }
            }
            TranscriptItem::Tool {
                call_id,
                name,
                arguments,
                parent_call_id,
            } => {
                let mut cell = ToolCell::new(call_id.clone(), name.clone());
                cell.depth = usize::from(parent_call_id.is_some());
                cell.status = ToolStatus::Completed;
                let arguments = serde_json::from_str::<serde_json::Value>(&arguments)
                    .unwrap_or(serde_json::Value::String(arguments));
                crate::events::describe_tool(&mut cell, &name, &arguments);
                let index = transcript.push(CellKind::Tool(cell));
                tools.insert(call_id, index);
            }
            TranscriptItem::ToolResult {
                call_id,
                output,
                outcome,
            } => {
                if let Some(index) = tools.get(&call_id).copied() {
                    transcript.update(index, |cell| {
                        if let CellKind::Tool(tool) = cell {
                            tool.status = match outcome {
                                nanocode::agent::session::ToolOutcome::Failed => ToolStatus::Failed,
                                _ => ToolStatus::Completed,
                            };
                            tool.output = crate::events::clean_output(&output);
                            if tool.output_body == ToolBody::Text
                                && crate::events::looks_like_diff(&tool.output)
                            {
                                tool.output_body = ToolBody::Diff;
                            }
                        }
                    });
                }
            }
        }
    }
}
