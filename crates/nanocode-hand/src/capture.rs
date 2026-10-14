use crate::Error;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Policy {
    max_dimension: u32,
    max_base64_bytes: usize,
    jpeg_qualities: Vec<u8>,
}
fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| {
        serde_json::from_str(include_str!("capture_policy.json"))
            .expect("valid embedded capture policy")
    })
}
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{RgbImage, codecs::jpeg::JpegEncoder};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Error>;

fn error(message: &str) -> Error {
    std::io::Error::other(message.to_owned()).into()
}

pub(crate) fn target_dimensions(width: f64, height: f64) -> Result<(u32, u32)> {
    if !width.is_finite()
        || !height.is_finite()
        || !(1.0..=65536.0).contains(&width)
        || !(1.0..=65536.0).contains(&height)
    {
        return Err(error(
            "main display is unavailable or has invalid dimensions",
        ));
    }
    let maximum = f64::from(policy().max_dimension);
    let scale = (maximum / width.max(height)).min(1.0);
    Ok((
        (width * scale).round().clamp(1.0, maximum) as u32,
        (height * scale).round().clamp(1.0, maximum) as u32,
    ))
}

pub(crate) fn encode_jpeg(frame: &RgbImage) -> Result<Value> {
    if !(1..=policy().max_dimension).contains(&frame.width())
        || !(1..=policy().max_dimension).contains(&frame.height())
    {
        return Err(error("unexpected screen capture dimensions"));
    }

    for &quality in &policy().jpeg_qualities {
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, quality)
            .encode_image(frame)
            .map_err(|_| error("could not encode screen JPEG"))?;
        if bytes.len().div_ceil(3) * 4 <= policy().max_base64_bytes {
            return Ok(
                json!({"status":"ok","jpeg":STANDARD.encode(bytes),"width":frame.width(),"height":frame.height()}),
            );
        }
    }
    Err(error("screen JPEG exceeds the 500000-byte transport limit"))
}
