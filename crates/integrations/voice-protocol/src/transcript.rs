use super::TranscriptEntry;

#[must_use]
pub fn project_transcript(text: &str, partial: bool) -> Option<Vec<TranscriptEntry>> {
    let text = text.trim();
    const TAGS: &[&str] = &[
        "realtime_delegation",
        "realtime_conversation",
        "source",
        "soruce",
        "startup_context",
    ];
    if partial && !text.is_empty() && TAGS.iter().any(|tag| format!("<{tag}").starts_with(text)) {
        return Some(Vec::new());
    }
    let tag = TAGS.iter().find(|tag| {
        text.strip_prefix('<')
            .and_then(|text| text.strip_prefix(**tag))
            .is_some_and(|tail| {
                tail.is_empty() || tail.starts_with('>') || tail.starts_with(char::is_whitespace)
            })
    })?;
    if *tag != "realtime_delegation" {
        return Some(Vec::new());
    }
    let Some(transcript) = field(text, "transcript_delta").filter(|text| !text.trim().is_empty())
    else {
        return Some(if text.contains("<source>") || text.contains("<soruce>") {
            Vec::new()
        } else {
            field(text, "input")
                .filter(|text| !text.trim().is_empty())
                .map(|text| vec![TranscriptEntry::new("user", decode(text))])
                .unwrap_or_default()
        });
    };
    let mut turns: Vec<TranscriptEntry> = Vec::new();
    for line in decode(transcript).lines() {
        if let Some((role, text)) = ["user", "assistant"].iter().find_map(|role| {
            line.strip_prefix(role)
                .and_then(|text| text.strip_prefix(':'))
                .map(|text| (*role, text.strip_prefix(' ').unwrap_or(text)))
        }) {
            turns.push(TranscriptEntry::new(role, text));
        } else if let Some(last) = turns.last_mut() {
            last.text.push('\n');
            last.text.push_str(line);
        } else if !line.trim().is_empty() {
            turns.push(TranscriptEntry::new("assistant", line));
        }
    }
    turns.retain(|turn| !turn.text.trim().is_empty());
    Some(turns)
}

fn field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.split_once(&format!("<{name}>"))?
        .1
        .split_once(&format!("</{name}>"))
        .map(|(value, _)| value)
}

fn decode(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
