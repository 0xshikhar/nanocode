use std::{borrow::Cow, env, path::Path};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn normalize_line_endings(text: &str) -> Cow<'_, str> {
    if !text.contains('\r') {
        return Cow::Borrowed(text);
    }

    let mut normalized = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(index) = remaining.find('\r') {
        normalized.push_str(&remaining[..index]);
        normalized.push('\n');
        remaining = &remaining[index + 1..];
        if let Some(after_newline) = remaining.strip_prefix('\n') {
            remaining = after_newline;
        }
    }
    normalized.push_str(remaining);
    Cow::Owned(normalized)
}

pub fn sanitize_terminal_text(text: &str) -> Cow<'_, str> {
    sanitize_terminal_text_with_break(text, '\n')
}

pub fn sanitize_terminal_text_inline(text: &str) -> Cow<'_, str> {
    sanitize_terminal_text_with_break(text, ' ')
}

fn sanitize_terminal_text_with_break(text: &str, line_break: char) -> Cow<'_, str> {
    let requires_sanitization = text.chars().any(|character| {
        character == '\r'
            || character == '\t'
            || character.is_control() && character != '\n'
            || character == '\n' && line_break != '\n'
    });
    if !requires_sanitization {
        return Cow::Borrowed(text);
    }

    let mut sanitized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\u{1b}' => skip_escape(&mut characters),
            '\u{9b}' => skip_control_sequence(&mut characters),
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                sanitized.push(line_break);
            }
            '\n' => sanitized.push(line_break),
            '\t' => sanitized.push_str("    "),
            character if character.is_control() => sanitized.push('�'),
            character => sanitized.push(character),
        }
    }
    Cow::Owned(sanitized)
}

fn skip_escape(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match characters.next() {
        Some('[') => skip_control_sequence(characters),
        Some(']' | 'P' | 'X' | '^' | '_') => {
            while let Some(character) = characters.next() {
                if character == '\u{7}' {
                    break;
                }
                if character == '\u{1b}' {
                    if characters.peek() == Some(&'\\') {
                        characters.next();
                    }
                    break;
                }
            }
        }
        Some(' '..='/') => {
            while characters
                .next_if(|character| (' '..='/').contains(character))
                .is_some()
            {}
            characters.next();
        }
        _ => {}
    }
}

fn skip_control_sequence(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while characters
        .next_if(|character| ('0'..='?').contains(character) || (' '..='/').contains(character))
        .is_some()
    {}
    characters.next_if(|character| ('@'..='~').contains(character));
}

pub fn terminal_text_width(text: &str) -> usize {
    text.graphemes(true)
        .map(|grapheme| match grapheme {
            "\t" => 4,
            grapheme if grapheme.contains(char::is_control) => 1,
            grapheme => grapheme.width(),
        })
        .sum()
}

pub fn format_duration(nanoseconds: u64) -> String {
    if nanoseconds >= 1_000_000_000 {
        let tenths = duration_display_tick(nanoseconds).saturating_sub(1_000);
        return format!("{}.{:01}s", tenths / 10, tenths % 10);
    }
    format!("{}ms", duration_display_tick(nanoseconds))
}

pub fn format_turn_duration(nanoseconds: u64) -> String {
    let total_seconds = nanoseconds / 1_000_000_000;
    let days = total_seconds / 86_400;
    let hours = total_seconds % 86_400 / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    parts.push(format!("{seconds}s"));
    parts.join(" ")
}

pub const fn duration_display_tick(nanoseconds: u64) -> u64 {
    if nanoseconds < 1_000_000_000 {
        return nanoseconds / 1_000_000;
    }
    1_000_u64.saturating_add(nanoseconds.saturating_add(50_000_000) / 100_000_000)
}

pub fn humanize_tool(name: &str) -> String {
    name.trim_start_matches("mcp__")
        .replace("__", " · ")
        .replace('_', " ")
}

pub fn shorten_home(path: &Path) -> String {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);
    let Some(home) = home else {
        return path.display().to_string();
    };
    if path == home {
        return "~".to_owned();
    }
    let Ok(relative) = path.strip_prefix(&home) else {
        return path.display().to_string();
    };
    format!("~/{}", relative.display())
}
