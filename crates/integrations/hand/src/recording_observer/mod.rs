use serde::Serialize;
use std::fmt;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveInput {
    Sensitive,

    Clear,

    Unknown,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ForegroundContext {
    pub app_id: String,
    pub window_id: String,
    pub process_id: Option<u32>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccessibilitySummary {
    pub role: String,
    pub subrole: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PointerSnapshot {
    pub x: i32,
    pub y: i32,

    pub buttons: u8,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservedEvent {
    FocusChanged,

    Button { button: u8, pressed: bool },

    Scroll { horizontal: i32, vertical: i32 },
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ObserverCapabilities {
    pub button_events: bool,
    pub scroll_events: bool,
    pub sensitive_input: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub foreground: ForegroundContext,
    pub pointer: PointerSnapshot,
    pub secure_input: SensitiveInput,
    pub accessibility: Option<AccessibilitySummary>,
    pub events: Vec<ObservedEvent>,
    pub capabilities: ObserverCapabilities,
}
#[derive(Debug, Clone, Serialize)]
pub struct ObserverError {
    pub code: &'static str,
    pub message: String,
}
impl ObserverError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl fmt::Display for ObserverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ObserverError {}
type Result<T> = std::result::Result<T, ObserverError>;

pub struct ObservedFrame {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub mime: String,
}

pub struct NativeObserver {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    inner: platform::Observer,
    previous: Option<(ForegroundContext, u8)>,
}
impl NativeObserver {
    pub fn new() -> Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            Ok(Self {
                inner: platform::Observer::new()?,
                previous: None,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err(ObserverError::new(
                "unsupported",
                "Native workflow observation is unavailable on this platform",
            ))
        }
    }

    pub fn for_desktop(runtime: Option<&std::path::Path>) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                inner: platform::Observer::for_desktop(runtime)?,
                previous: None,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            if runtime.is_some() {
                return Err(ObserverError::new(
                    "unsupported_desktop",
                    "Private X11 runtime is supported only on Linux",
                ));
            }
            Self::new()
        }
    }

    pub fn frame(&mut self, expected: &ForegroundContext) -> Result<ObservedFrame> {
        #[cfg(target_os = "linux")]
        {
            let (before, before_sensitive) = self.inner.context()?;
            if before != *expected || before_sensitive != SensitiveInput::Clear {
                return Err(ObserverError::new(
                    "capture_suppressed",
                    "Foreground changed or sensitive-input safety is not verified",
                ));
            }
            let frame = self.inner.frame(expected)?;
            let (after, after_sensitive) = self.inner.context()?;
            if after != *expected || after_sensitive != SensitiveInput::Clear {
                return Err(ObserverError::new(
                    "capture_suppressed",
                    "Foreground or sensitive-input context changed during capture",
                ));
            }
            Ok(frame)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = expected;
            Err(ObserverError::new(
                "capture_unsupported",
                "Selected-window capture is not implemented for this native adapter",
            ))
        }
    }

    pub fn sample(&mut self) -> Result<Observation> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            let mut observation = match self.inner.sample() {
                Ok(value) => value,
                Err(error) => {
                    self.previous = None;
                    return Err(error);
                }
            };
            if let Some((foreground, buttons)) = &self.previous {
                if foreground != &observation.foreground {
                    observation.events.clear();
                    observation.events.push(ObservedEvent::FocusChanged);
                }
                if foreground == &observation.foreground && !self.inner.native_button_events() {
                    for bit in 0..3 {
                        if (buttons ^ observation.pointer.buttons) & (1 << bit) != 0 {
                            observation.events.push(ObservedEvent::Button {
                                button: bit + 1,
                                pressed: observation.pointer.buttons & (1 << bit) != 0,
                            });
                        }
                    }
                }
            }
            self.previous = Some((observation.foreground.clone(), observation.pointer.buttons));
            Ok(observation)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err(ObserverError::new(
                "unsupported",
                "Native workflow observation is unavailable on this platform",
            ))
        }
    }
}
