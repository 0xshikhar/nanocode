use std::time::{Duration, Instant};

use nanocode_tui::{
    keybindings::{self, Binding},
    ratatui::{
        Frame,
        layout::{Constraint, Layout, Rect},
        style::{Modifier, Style},
        text::{Line, Span},
        widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
    },
    spinner::elapsed_label,
};

use crate::app::{App, Overlay, TurnState};

pub const STATUS_VISIBLE: Duration = Duration::from_secs(4);

pub const BINDINGS: [Binding; 26] = [
    Binding {
        keys: "enter",
        action: "send · queue while a turn runs",
    },
    Binding {
        keys: "enter on empty prompt",
        action: "steer the queued message into the running turn",
    },
    Binding {
        keys: "tab",
        action: "queue a follow-up while a turn runs",
    },
    Binding {
        keys: "shift+enter / ctrl+j",
        action: "insert a newline",
    },
    Binding {
        keys: "esc esc",
        action: "interrupt the running turn",
    },
    Binding {
        keys: "alt+u / alt+up",
        action: "undo the latest queued or steer message",
    },
    Binding {
        keys: "@path",
        action: "fuzzy file finder (tab/enter inserts)",
    },
    Binding {
        keys: "!command",
        action: "run a shell command locally",
    },
    Binding {
        keys: "ctrl+v",
        action: "attach an image from the clipboard",
    },
    Binding {
        keys: "up / down",
        action: "prompt history",
    },
    Binding {
        keys: "ctrl+r",
        action: "recent prompts",
    },
    Binding {
        keys: "ctrl+g",
        action: "edit the prompt in $EDITOR",
    },
    Binding {
        keys: "ctrl+z",
        action: "restore the cleared draft",
    },
    Binding {
        keys: "ctrl+d",
        action: "model picker",
    },
    Binding {
        keys: "ctrl+s",
        action: "thinking dial",
    },
    Binding {
        keys: "ctrl+o",
        action: "expand or collapse tools and reasoning",
    },
    Binding {
        keys: "ctrl+t",
        action: "toggle the subagent tree",
    },
    Binding {
        keys: "pgup / pgdn · wheel",
        action: "scroll the transcript",
    },
    Binding {
        keys: "ctrl+home / ctrl+end",
        action: "transcript top / follow",
    },
    Binding {
        keys: "ctrl+l",
        action: "redraw",
    },
    Binding {
        keys: "ctrl+c",
        action: "clear draft · twice to quit",
    },
    Binding {
        keys: "/model /thinking /fast",
        action: "harness, model, thinking, speed",
    },
    Binding {
        keys: "/btw <question>",
        action: "side question while working",
    },
    Binding {
        keys: "/resume · /new",
        action: "switch sessions",
    },
    Binding {
        keys: "/theme /copy /context /skills",
        action: "theme, copy reply, usage, skills",
    },
    Binding {
        keys: "F1 · /help",
        action: "this help",
    },
];

pub fn draw(app: &mut App, frame: &mut Frame<'_>) {
    let area = frame.area();
    let now = Instant::now();
    let spinner = app.spinner.frame(now);
    let theme = app.theme.clone();
    let width = area.width;
    let composer_height = app
        .composer
        .height(width.saturating_sub(2))
        .clamp(1, (area.height / 3).max(1))
        + 2;
    let mut queue_lines = app.queue.lines(width, &theme);
    if !app.attachments.is_empty() {
        let names = app
            .attachments
            .iter()
            .map(|path| {
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        queue_lines.push(Line::from(vec![
            Span::styled("⎘ attached: ", Style::default().fg(theme.accent())),
            Span::styled(names, Style::default().fg(theme.text())),
            Span::styled("  · /detach removes", Style::default().fg(theme.muted())),
        ]));
    }
    let tree_lines = if app.show_tree && !app.tree.is_empty() {
        app.tree.lines(&theme, spinner, None)
    } else {
        Vec::new()
    };
    let tree_height = if tree_lines.is_empty() {
        0
    } else {
        u16::try_from(tree_lines.len() + 1).unwrap_or(u16::MAX)
    };
    let [
        transcript_area,
        tree_area,
        queue_area,
        status_area,
        composer_area,
        footer_area,
    ] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(tree_height),
        Constraint::Length(u16::try_from(queue_lines.len()).unwrap_or(u16::MAX)),
        Constraint::Length(1),
        Constraint::Length(composer_height),
        Constraint::Length(1),
    ])
    .areas(area);
    let buffer = frame.buffer_mut();
    app.transcript
        .render(transcript_area, buffer, &theme, spinner);
    if tree_height > 0 {
        let mut lines = vec![Line::styled(
            format!("agents · {} active · ctrl+t to hide", app.tree.active()),
            Style::default()
                .fg(theme.muted())
                .add_modifier(Modifier::BOLD),
        )];
        lines.extend(tree_lines);
        Paragraph::new(lines).render(tree_area, buffer);
    }
    Paragraph::new(queue_lines).render(queue_area, buffer);
    status_line(app, spinner, now).render(status_area, buffer);
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border()));
    let inner = block.inner(composer_area);
    block.render(composer_area, buffer);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    let cursor = app
        .composer
        .render(inner, buffer, &theme, app.overlay.is_none());
    footer(app).render(footer_area, buffer);
    command_popup(app, composer_area, transcript_area, frame);
    finder_popup(app, composer_area, transcript_area, frame);
    if let Some(cursor) = cursor {
        frame.set_cursor_position(cursor);
    }
    match &mut app.overlay {
        Some(Overlay::Picker(_, picker)) => picker.render(area, frame.buffer_mut(), &theme),
        Some(Overlay::Help) => keybindings::render(&BINDINGS, area, frame.buffer_mut(), &theme),
        Some(Overlay::Dial { dial, .. }) => dial.render(area, frame.buffer_mut(), &theme),
        None => {}
    }
}

pub const FINDER_ROWS: usize = 8;

fn finder_popup(app: &App, composer_area: Rect, transcript_area: Rect, frame: &mut Frame<'_>) {
    let Some(finder) = &app.finder else {
        return;
    };
    if app.overlay.is_some() {
        return;
    }
    let rows = FINDER_ROWS.min(usize::from(transcript_area.height.saturating_sub(1)));
    let lines = finder.finder.lines(rows, &app.theme);
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let popup = Rect {
        x: composer_area.x,
        y: composer_area.y.saturating_sub(height + 1),
        width: composer_area.width,
        height: height + 1,
    };
    let buffer = frame.buffer_mut();
    Clear.render(popup, buffer);
    let mut all = vec![Line::styled(
        format!(
            "  files matching @{} · {} of {} · tab/enter insert · esc close",
            finder.finder.query(),
            finder.finder.matches(),
            finder.finder.len()
        ),
        Style::default().fg(app.theme.muted()),
    )];
    all.extend(lines);
    Paragraph::new(all).render(popup, buffer);
}

fn command_popup(app: &App, composer_area: Rect, transcript_area: Rect, frame: &mut Frame<'_>) {
    let text = app.composer.text();
    if app.overlay.is_some() || !text.starts_with('/') || text.contains(' ') || text.contains('\n')
    {
        return;
    }
    let theme = &app.theme;
    let matches = crate::commands::matching(text).collect::<Vec<_>>();
    if matches.is_empty() {
        return;
    }
    let height = u16::try_from(matches.len())
        .unwrap_or(u16::MAX)
        .min(transcript_area.height);
    let popup = Rect {
        x: composer_area.x,
        y: composer_area.y.saturating_sub(height + 1),
        width: composer_area.width,
        height: height + 1,
    };
    let lines = matches
        .into_iter()
        .enumerate()
        .map(|(index, (command, description))| {
            let style = if index == 0 {
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text())
            };
            Line::from(vec![
                Span::styled(format!("  {command:<12}"), style),
                Span::styled(description, Style::default().fg(theme.muted())),
            ])
        })
        .collect::<Vec<_>>();
    let buffer = frame.buffer_mut();
    Clear.render(popup, buffer);
    let mut all = vec![Line::styled(
        "  tab to complete",
        Style::default().fg(theme.muted()),
    )];
    all.extend(lines);
    Paragraph::new(all).render(popup, buffer);
}

fn status_line(app: &App, spinner: &str, now: Instant) -> Line<'static> {
    let theme = &app.theme;
    if let Some((text, at)) = &app.status
        && now.duration_since(*at) < STATUS_VISIBLE
    {
        return Line::styled(
            format!(" {text}"),
            Style::default().fg(theme.thinking_high()),
        );
    }
    if app.interrupting && !matches!(app.turn, TurnState::Idle) {
        return Line::from(vec![
            Span::styled(
                format!(" {spinner} "),
                Style::default().fg(theme.thinking_max()),
            ),
            Span::styled("interrupting…", Style::default().fg(theme.text())),
        ]);
    }
    match &app.turn {
        TurnState::Running { since, .. }
        | TurnState::Starting(since)
        | TurnState::External(since) => Line::from(vec![
            Span::styled(format!(" {spinner} "), Style::default().fg(theme.accent())),
            Span::styled(
                "Working",
                Style::default()
                    .fg(theme.text())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " ({} · {} · enter queues · enter on empty steers)",
                    elapsed_label(now.duration_since(*since)),
                    if matches!(app.turn, TurnState::External(_)) {
                        "woken by a finished subagent · esc esc to interrupt"
                    } else {
                        "esc esc to interrupt"
                    }
                ),
                Style::default().fg(theme.muted()),
            ),
        ]),
        TurnState::Idle if app.starting => Line::from(vec![
            Span::styled(format!(" {spinner} "), Style::default().fg(theme.accent())),
            Span::styled("connecting…", Style::default().fg(theme.muted())),
        ]),
        TurnState::Idle => Line::styled(
            " F1 for keys · / for commands",
            Style::default().fg(theme.muted()),
        ),
    }
}

fn footer(app: &App) -> Line<'static> {
    let theme = &app.theme;
    let mut spans = vec![
        Span::styled(
            format!(" {} ", crate::app::family_label(&app.model.family())),
            Style::default()
                .fg(theme.code_background())
                .bg(theme.accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", app.model),
            Style::default()
                .fg(theme.text())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", app.thinking),
            Style::default().fg(theme.muted()),
        ),
    ];
    if app.tier != nanocode::ServiceTier::Standard {
        spans.push(Span::styled(
            format!(" · {}", app.tier.as_str()),
            Style::default().fg(theme.thinking_high()),
        ));
    }
    if let Some((model, thinking, _)) = &app.pending_switch {
        spans.push(Span::styled(
            format!(" → {model} {thinking} after turn"),
            Style::default().fg(theme.thinking_medium()),
        ));
    }
    if let Some(session) = app.session_id() {
        let short = session
            .get(session.len().saturating_sub(8)..)
            .unwrap_or(&session)
            .to_owned();
        spans.push(Span::styled(
            format!(" · {short}"),
            Style::default().fg(theme.muted()),
        ));
    }
    if app.host.config.store.is_none() {
        spans.push(Span::styled(
            " · ephemeral",
            Style::default().fg(theme.muted()),
        ));
    }
    if let Some(remote) = &app.host.config.remote {
        spans.push(Span::styled(
            format!(" · runner {}", remote.status_label()),
            Style::default().fg(theme.muted()),
        ));
    }
    spans.push(Span::styled(
        format!(
            " · {}",
            nanocode_tui::format::shorten_home(&app.host.config.workspace)
        ),
        Style::default().fg(theme.muted()),
    ));
    Line::from(spans)
}

pub fn splash(frame: &mut Frame<'_>, theme: &nanocode_tui::theme::Theme, message: &str) {
    let area = frame.area();
    let y = area.y + area.height / 2;
    let line = Line::from(vec![
        Span::styled(
            "nanocodex ",
            Style::default()
                .fg(theme.accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(message.to_owned(), Style::default().fg(theme.muted())),
    ]);
    Paragraph::new(line)
        .alignment(nanocode_tui::ratatui::layout::Alignment::Center)
        .render(
            Rect {
                y,
                height: 1,
                ..area
            },
            frame.buffer_mut(),
        );
}
