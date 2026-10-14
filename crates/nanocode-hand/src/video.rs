//! Shared H.264 output geometry, limits, and level selection.
use crate::Error;
type Result<T> = std::result::Result<T, Error>;
fn error(message: &str) -> Error {
    std::io::Error::other(message.to_owned()).into()
}

/// Bounded, aspect-preserving video settings for a 60 Hz display source.
#[derive(Debug)]
pub struct VideoSettings {
    /// Encoded width, even and never larger than the source.
    pub width: u32,
    /// Encoded height, even and never larger than the source.
    pub height: u32,
    /// Target H.264 bitrate in kilobits per second.
    pub bitrate_kbps: u32,
    /// H.264 level covering the output's macroblock rate and bitrate.
    pub level: &'static str,
}
impl VideoSettings {
    /// Apply common validated environment overrides to platform defaults.
    pub fn from_environment(
        width: u32,
        height: u32,
        default_max_dimension: u32,
        default_bitrate: u32,
    ) -> Result<Self> {
        let maximum = video_setting(
            "NANOCODEX_SCREEN_MAX_DIMENSION",
            default_max_dimension,
            1280,
            7680,
        )?;
        let mut settings = Self::new(width, height, maximum, default_bitrate)?;
        // Budget the default for the pixels actually sent. 24 Mbps is useful
        // for 4K, but at 1080p it can needlessly queue video on a WAN uplink.
        // An explicit bitrate remains an exact override.
        let suggested = default_bitrate_for(&settings, default_max_dimension, default_bitrate);
        settings.bitrate_kbps =
            video_setting("NANOCODEX_SCREEN_BITRATE_KBPS", suggested, 1000, 100000)?;
        settings.level = video_level(settings.width, settings.height, settings.bitrate_kbps);
        Ok(settings)
    }
    fn new(width: u32, height: u32, maximum: u32, bitrate_kbps: u32) -> Result<Self> {
        if !(2..=65536).contains(&width) || !(2..=65536).contains(&height) {
            return Err(error("invalid video display dimensions"));
        }
        let scale = (f64::from(maximum) / f64::from(width.max(height))).min(1.0);
        let width = ((f64::from(width) * scale) as u32 / 2 * 2).max(2);
        let height = ((f64::from(height) * scale) as u32 / 2 * 2).max(2);
        Ok(Self {
            width,
            height,
            bitrate_kbps,
            level: video_level(width, height, bitrate_kbps),
        })
    }
}
fn default_bitrate_for(settings: &VideoSettings, max_dimension: u32, bitrate: u32) -> u32 {
    // Platform defaults describe a 16:9 frame at the default longest side.
    let reference_pixels = u64::from(max_dimension) * u64::from(max_dimension) * 9 / 16;
    let pixels = u64::from(settings.width) * u64::from(settings.height);
    (u64::from(bitrate) * pixels / reference_pixels)
        .max(1000)
        .min(u64::from(bitrate)) as u32
}
fn video_setting(name: &str, default: u32, min: u32, max: u32) -> Result<u32> {
    let value = match std::env::var(name) {
        Ok(value) => value.parse::<u32>()?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    if !(min..=max).contains(&value) {
        return Err(error(&format!("{name} must be between {min} and {max}")));
    }
    Ok(value)
}

pub(crate) fn video_level(width: u32, height: u32, bitrate_kbps: u32) -> &'static str {
    let macroblocks_per_second =
        u64::from(width.div_ceil(16)) * u64::from(height.div_ceil(16)) * 60;
    match macroblocks_per_second {
        0..=216000 if bitrate_kbps <= 20000 => "3.2",
        0..=522240 if bitrate_kbps <= 50000 => "4.2",
        0..=983040 => "5.1",
        983041..=2073600 => "5.2",
        2073601..=4177920 => "6.0",
        4177921..=8355840 => "6.1",
        _ => "6.2",
    }
}
