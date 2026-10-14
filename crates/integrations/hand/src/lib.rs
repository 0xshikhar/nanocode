#![deny(unsafe_code)]

pub type Error = Box<dyn std::error::Error + Send + Sync>;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod capture;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    access_status, main_display_index, main_display_pixel_dimensions, request, request_access,
};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{ensure_interactive_session, request, video_command};

#[cfg(target_os = "windows")]
mod windows_audio;
#[cfg(target_os = "windows")]
pub use windows_audio::capture_audio;

#[cfg(target_os = "macos")]
mod macos_audio;
#[cfg(target_os = "macos")]
pub use macos_audio::{capture_audio, capture_video, main_display_dimensions};
mod video;
pub use video::VideoSettings;

pub mod recording;

pub mod recording_observer;
