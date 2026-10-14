use std::{rc::Rc, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use syntect::easy::HighlightLines;

use crate::{
    diff,
    format::{format_duration, sanitize_terminal_text, sanitize_terminal_text_inline},
    highlight, markdown,
    theme::Theme,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolBody {
    Text,
    Code,
    Diff,
}

#[derive(Clone, Debug)]
pub struct ToolCell {
    pub call_id: String,
    pub name: String,
    pub title: String,
    pub input: String,
    pub input_body: ToolBody,
    pub output: String,
    pub output_body: ToolBody,
    pub status: ToolStatus,
    pub duration: Option<Duration>,
    pub depth: usize,
}

impl ToolCell {
    #[must_use]
    pub fn new(call_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            name: name.into(),
            title: String::new(),
            input: String::new(),
            input_body: ToolBody::Text,
            output: String::new(),
            output_body: ToolBody::Text,
            status: ToolStatus::Running,
            duration: None,
            depth: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum CellKind {
    User(String),
    Assistant(String),
    Reasoning(String),
    Tool(ToolCell),
    Notice(NoticeLevel, String),
    Btw {
        question: String,
        answer: String,
        done: bool,
    },
    Summary(String),
}

#[derive(Clone, Debug)]
struct Cell {
    kind: CellKind,
    revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CacheKey {
    width: u16,
    revision: u64,
    expanded: bool,
    reasoning: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct TranscriptPolicy {
    pub collapsed_preview_lines: usize,
    pub collapsed_diff_lines: usize,
}

pub struct Transcript {
    cells: Vec<Cell>,
    cache: Vec<Option<(CacheKey, Rc<Vec<Line<'static>>>)>>,
    revision: u64,
    scroll: usize,
    expanded: bool,
    reasoning: bool,
    policy: TranscriptPolicy,
    last_height: usize,
    last_total: usize,
}

impl Transcript {
    #[must_use]
    pub const fn new(policy: TranscriptPolicy) -> Self {
        Self {
            cells: Vec::new(),
            cache: Vec::new(),
            revision: 0,
            scroll: 0,
            expanded: false,
            reasoning: false,
            policy,
            last_height: 0,
            last_total: 0,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.cells.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.cache.clear();
        self.scroll = 0;
    }

    pub fn push(&mut self, kind: CellKind) -> usize {
        self.revision += 1;
        self.cells.push(Cell {
            kind,
            revision: self.revision,
        });
        self.cache.push(None);
        self.cells.len() - 1
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<&CellKind> {
        self.cells.get(index).map(|cell| &cell.kind)
    }

    pub fn update(&mut self, index: usize, change: impl FnOnce(&mut CellKind)) -> bool {
        let Some(cell) = self.cells.get_mut(index) else {
            return false;
        };
        change(&mut cell.kind);
        self.revision += 1;
        cell.revision = self.revision;
        true
    }

    #[must_use]
    pub fn last(&self) -> Option<(usize, &CellKind)> {
        self.cells
            .last()
            .map(|cell| (self.cells.len() - 1, &cell.kind))
    }

    #[must_use]
    pub fn find_tool(&self, call_id: &str) -> Option<usize> {
        self.cells
            .iter()
            .rposition(|cell| matches!(&cell.kind, CellKind::Tool(tool) if tool.call_id == call_id))
    }

    pub fn finish_running_tools(&mut self, status: ToolStatus) {
        for index in 0..self.cells.len() {
            if matches!(&self.cells[index].kind, CellKind::Tool(tool) if tool.status == ToolStatus::Running)
            {
                self.update(index, |kind| {
                    if let CellKind::Tool(tool) = kind {
                        tool.status = status;
                    }
                });
            }
        }
    }

    #[must_use]
    pub const fn expanded(&self) -> bool {
        self.expanded
    }

    pub const fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }

    #[must_use]
    pub const fn shows_reasoning(&self) -> bool {
        self.reasoning
    }

    pub const fn toggle_reasoning(&mut self) {
        self.reasoning = !self.reasoning;
    }

    #[must_use]
    pub const fn is_following(&self) -> bool {
        self.scroll == 0
    }

    pub const fn scroll_up(&mut self, lines: usize) {
        let limit = self.last_total.saturating_sub(self.last_height);
        self.scroll = if self.scroll + lines > limit {
            limit
        } else {
            self.scroll + lines
        };
    }

    pub const fn scroll_down(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_sub(lines);
    }

    pub const fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
    }

    pub const fn scroll_to_top(&mut self) {
        self.scroll = self.last_total.saturating_sub(self.last_height);
    }

    #[must_use]
    pub const fn page(&self) -> usize {
        if self.last_height > 2 {
            self.last_height - 2
        } else {
            1
        }
    }

    fn cell_lines(
        &mut self,
        index: usize,
        width: u16,
        theme: &Theme,
        spinner: &str,
    ) -> Rc<Vec<Line<'static>>> {
        let cell = &self.cells[index];
        let running = matches!(&cell.kind, CellKind::Tool(tool) if tool.status == ToolStatus::Running)
            || matches!(&cell.kind, CellKind::Btw { done: false, .. });
        let key = CacheKey {
            width,
            revision: cell.revision,
            expanded: self.expanded,
            reasoning: self.reasoning,
        };
        if !running
            && let Some((cached, lines)) = &self.cache[index]
            && *cached == key
        {
            return Rc::clone(lines);
        }
        let mut lines = Vec::new();
        if index > 0 {
            lines.push(Line::default());
        }
        let context = RenderContext {
            width,
            theme,
            spinner,
            expanded: self.expanded,
            reasoning: self.reasoning || self.expanded,
            preview: self.policy.collapsed_preview_lines,
            diff_preview: self.policy.collapsed_diff_lines,
        };
        context.render(&cell.kind, &mut lines);
        let lines = Rc::new(lines);
        if !running {
            self.cache[index] = Some((key, Rc::clone(&lines)));
        }
        lines
    }

    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &Theme, spinner: &str) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let width = area.width;
        let height = usize::from(area.height);
        let rendered = (0..self.cells.len())
            .map(|index| self.cell_lines(index, width, theme, spinner))
            .collect::<Vec<_>>();
        let total = rendered.iter().map(|lines| lines.len()).sum::<usize>();
        if self.scroll > 0 && total > self.last_total && self.last_total > 0 {
            self.scroll += total - self.last_total;
        }
        self.last_total = total;
        self.last_height = height;
        self.scroll = self.scroll.min(total.saturating_sub(height));
        let end = total - self.scroll;
        let start = end.saturating_sub(height);
        let mut visible = Vec::with_capacity(end - start);
        let mut offset = 0;
        for lines in &rendered {
            let next = offset + lines.len();
            if next > start && offset < end {
                let from = start.saturating_sub(offset);
                let to = (end - offset).min(lines.len());
                visible.extend(lines[from..to].iter().cloned());
            }
            offset = next;
            if offset >= end {
                break;
            }
        }
        Paragraph::new(visible).render(area, buffer);
        if self.scroll > 0 {
            let label = format!(" ↓ {} more lines · end to follow ", self.scroll);
            let label_width = u16::try_from(label.chars().count())
                .unwrap_or(u16::MAX)
                .min(area.width);
            let x = area.x + area.width - label_width;
            buffer.set_stringn(
                x,
                area.y + area.height - 1,
                &label,
                usize::from(label_width),
                Style::default()
                    .fg(theme.code_background())
                    .bg(theme.accent()),
            );
        }
    }
}

struct Gutter {
    first: String,
    rest: String,
    style: Style,
}

struct RenderContext<'a> {
    width: u16,
    theme: &'a Theme,
    spinner: &'a str,
    expanded: bool,
    reasoning: bool,
    preview: usize,
    diff_preview: usize,
}

impl RenderContext<'_> {
    fn render(&self, kind: &CellKind, lines: &mut Vec<Line<'static>>) {
        let theme = self.theme;
        match kind {
            CellKind::User(text) => {
                let body = markdown::wrap_plain_preserving_whitespace(
                    &sanitize_terminal_text(text),
                    self.width.saturating_sub(2),
                    Style::default()
                        .fg(theme.text())
                        .add_modifier(Modifier::BOLD),
                );
                prefixed(
                    lines,
                    body,
                    Span::styled(
                        "› ",
                        Style::default()
                            .fg(theme.accent())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                );
            }
            CellKind::Assistant(text) => {
                let body = markdown::render(text, self.width.saturating_sub(2), theme).lines;
                prefixed(
                    lines,
                    body,
                    Span::styled("● ", Style::default().fg(theme.text())),
                    Span::raw("  "),
                );
            }
            CellKind::Reasoning(text) => {
                let style = Style::default()
                    .fg(theme.muted())
                    .add_modifier(Modifier::ITALIC);
                if self.reasoning {
                    let body = markdown::wrap_plain(
                        &sanitize_terminal_text(text.trim()),
                        self.width.saturating_sub(2),
                        style,
                    );
                    prefixed(lines, body, Span::styled("∴ ", style), Span::raw("  "));
                } else {
                    let summary = sanitize_terminal_text_inline(
                        text.trim().lines().next().unwrap_or_default(),
                    )
                    .into_owned();
                    let summary = summary.trim_matches('*').trim().to_owned();
                    let mut collapsed = vec![Span::styled("∴ thinking", style)];
                    if !summary.is_empty() {
                        collapsed.push(Span::styled(format!(" · {summary}"), style));
                    }
                    collapsed.push(Span::styled(
                        "  (ctrl+o)",
                        Style::default().fg(theme.border()),
                    ));
                    let line = Line::from(collapsed);
                    lines.extend(
                        markdown::wrap_spans(&line.spans, self.width, true)
                            .into_iter()
                            .take(1),
                    );
                }
            }
            CellKind::Tool(tool) => self.tool(tool, lines),
            CellKind::Notice(level, text) => {
                let (icon, color) = match level {
                    NoticeLevel::Info => ("ℹ ", theme.muted()),
                    NoticeLevel::Warning => ("⚠ ", theme.thinking_high()),
                    NoticeLevel::Error => ("✗ ", theme.thinking_max()),
                };
                let body = markdown::wrap_plain(
                    &sanitize_terminal_text(text),
                    self.width.saturating_sub(2),
                    Style::default().fg(color),
                );
                prefixed(
                    lines,
                    body,
                    Span::styled(icon, Style::default().fg(color)),
                    Span::raw("  "),
                );
            }
            CellKind::Btw {
                question,
                answer,
                done,
            } => {
                let bar = Style::default().fg(theme.thinking_medium());
                let header = if *done {
                    "btw".to_owned()
                } else {
                    format!("btw {}", self.spinner)
                };
                lines.push(Line::from(vec![
                    Span::styled("┃ ", bar),
                    Span::styled(header, bar.add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("  {}", sanitize_terminal_text_inline(question)),
                        Style::default().fg(theme.muted()),
                    ),
                ]));
                if !answer.is_empty() {
                    let body = markdown::render(answer, self.width.saturating_sub(2), theme).lines;
                    prefixed(
                        lines,
                        body,
                        Span::styled("┃ ", bar),
                        Span::styled("┃ ", bar),
                    );
                }
            }
            CellKind::Summary(text) => {
                let label = format!(" {} ", sanitize_terminal_text_inline(text));
                let fill = usize::from(self.width).saturating_sub(label.chars().count() + 2);
                lines.push(Line::from(vec![
                    Span::styled("──", Style::default().fg(theme.border())),
                    Span::styled(label, Style::default().fg(theme.muted())),
                    Span::styled("─".repeat(fill), Style::default().fg(theme.border())),
                ]));
            }
        }
    }

    fn tool(&self, tool: &ToolCell, lines: &mut Vec<Line<'static>>) {
        let theme = self.theme;
        let indent = "  ".repeat(tool.depth);
        let (icon, color) = match tool.status {
            ToolStatus::Running => (self.spinner, theme.accent()),
            ToolStatus::Completed => ("✓", theme.thinking_low()),
            ToolStatus::Failed => ("✗", theme.thinking_max()),
            ToolStatus::Cancelled => ("⊘", theme.muted()),
        };
        let mut header = vec![
            Span::raw(indent.clone()),
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::styled(
                tool.name.clone(),
                Style::default()
                    .fg(theme.text())
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if !tool.title.is_empty() {
            header.push(Span::styled(
                format!(" {}", sanitize_terminal_text_inline(&tool.title)),
                Style::default().fg(theme.muted()),
            ));
        }
        if let Some(duration) = tool.duration {
            header.push(Span::styled(
                format!(
                    " · {}",
                    format_duration(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX))
                ),
                Style::default().fg(theme.muted()),
            ));
        }
        lines.push(Line::from(header));
        let inner = self
            .width
            .saturating_sub(u16::try_from(indent.len() + 4).unwrap_or(u16::MAX));
        let gutter = Style::default().fg(theme.border());
        if !tool.input.trim().is_empty() {
            let body = self.body(&tool.input, tool.input_body, inner);
            let limit = self.limit(tool.input_body);
            let gutter = Gutter {
                first: format!("{indent}  │ "),
                rest: format!("{indent}  │ "),
                style: gutter,
            };
            self.preview(lines, body, limit, &gutter, false);
        }
        if !tool.output.trim().is_empty() {
            let body = self.body(&tool.output, tool.output_body, inner);
            let limit = self.limit(tool.output_body);
            let gutter = Gutter {
                first: format!("{indent}  └ "),
                rest: format!("{indent}    "),
                style: gutter,
            };
            self.preview(lines, body, limit, &gutter, true);
        }
    }

    fn body(&self, text: &str, body: ToolBody, width: u16) -> Vec<Line<'static>> {
        let text = sanitize_terminal_text(text.trim_end());
        match body {
            ToolBody::Diff => diff::render(&text, width, self.theme),
            ToolBody::Code => code_lines(&text, "js", width),
            ToolBody::Text => markdown::wrap_plain_preserving_whitespace(
                &text,
                width,
                Style::default().fg(self.theme.muted()),
            ),
        }
    }

    const fn limit(&self, body: ToolBody) -> usize {
        match body {
            ToolBody::Diff => self.diff_preview,
            ToolBody::Text | ToolBody::Code => self.preview,
        }
    }

    fn preview(
        &self,
        lines: &mut Vec<Line<'static>>,
        body: Vec<Line<'static>>,
        preview: usize,
        gutter: &Gutter,
        tail: bool,
    ) {
        let total = body.len();
        let limit = if self.expanded {
            total
        } else {
            preview.min(total)
        };
        let hidden = total - limit;
        let mut shown: Vec<Line<'static>> = if tail {
            body.into_iter().skip(hidden).collect()
        } else {
            body.into_iter().take(limit).collect()
        };
        if hidden > 0 {
            let hint = Line::styled(
                format!("… {hidden} more lines (ctrl+o to expand)"),
                Style::default()
                    .fg(self.theme.muted())
                    .add_modifier(Modifier::ITALIC),
            );
            if tail {
                shown.insert(0, hint);
            } else {
                shown.push(hint);
            }
        }
        prefixed(
            lines,
            shown,
            Span::styled(gutter.first.clone(), gutter.style),
            Span::styled(gutter.rest.clone(), gutter.style),
        );
    }
}

fn code_lines(text: &str, language: &str, width: u16) -> Vec<Line<'static>> {
    let assets = highlight::assets();
    let syntax = highlight::syntax_for_token(&assets.syntaxes, language);
    let mut highlighter = HighlightLines::new(syntax, highlight::theme());
    text.lines()
        .flat_map(|line| {
            highlight::wrap(
                highlight::line(&mut highlighter, line, &assets.syntaxes),
                width,
            )
            .into_iter()
            .map(Line::from)
        })
        .collect()
}

fn prefixed(
    lines: &mut Vec<Line<'static>>,
    body: Vec<Line<'static>>,
    first: Span<'static>,
    rest: Span<'static>,
) {
    for (index, line) in body.into_iter().enumerate() {
        let prefix = if index == 0 {
            first.clone()
        } else {
            rest.clone()
        };
        let mut spans = Vec::with_capacity(line.spans.len() + 1);
        spans.push(prefix);
        spans.extend(line.spans);
        lines.push(Line::from(spans).style(line.style));
    }
}
