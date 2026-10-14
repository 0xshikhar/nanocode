pub const COMMANDS: [(&str, &str); 16] = [
    ("/model", "switch harness, model, thinking and speed"),
    ("/thinking", "change the thinking level"),
    (
        "/fast",
        "cycle the service tier (standard, fast, ultrafast)",
    ),
    ("/btw", "ask a side question without interrupting the turn"),
    ("/resume", "resume a saved session"),
    ("/new", "start a new session"),
    ("/agents", "toggle the subagent tree"),
    ("/compact", "compact the conversation context"),
    ("/session", "show session, model and tool details"),
    ("/expand", "expand or collapse tools and reasoning"),
    ("/effort", "change the thinking level"),
    ("/mouse", "toggle mouse capture for native text selection"),
    ("/help", "show key bindings"),
    ("/clear", "start a new session"),
    ("/status", "show session, model and tool details"),
    ("/quit", "exit nanocodex"),
];

pub fn matching(prefix: &str) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
    COMMANDS
        .iter()
        .copied()
        .filter(move |(command, _)| command.starts_with(prefix))
}

pub fn complete(prefix: &str) -> Option<&'static str> {
    matching(prefix).next().map(|(command, _)| command)
}
