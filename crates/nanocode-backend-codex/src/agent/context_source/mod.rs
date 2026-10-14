#[cfg(not(target_family = "wasm"))]
mod agents_md;

#[cfg(not(target_family = "wasm"))]
#[path = "native.rs"]
mod platform;

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
#[path = "web.rs"]
mod platform;

#[derive(Clone, Debug)]
pub struct ExecutionEnvironment {
    pub(crate) current_date: std::sync::Arc<str>,
    pub(crate) timezone: std::sync::Arc<str>,
    pub(crate) project_instructions: Option<std::sync::Arc<str>>,
}

impl ExecutionEnvironment {
    #[must_use]
    pub fn new(
        current_date: impl Into<std::sync::Arc<str>>,
        timezone: impl Into<std::sync::Arc<str>>,
    ) -> Self {
        Self {
            current_date: current_date.into(),
            timezone: timezone.into(),
            project_instructions: None,
        }
    }

    #[must_use]
    pub fn project_instructions(mut self, instructions: impl Into<std::sync::Arc<str>>) -> Self {
        self.project_instructions = Some(instructions.into());
        self
    }
}

pub(crate) use platform::ContextSource;
pub(super) use platform::ContextSourceConfig;
