use std::borrow::Cow;

use image::ColorType;

use super::ImagePreparationError;

pub(super) fn png_exif(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 8usize;
    loop {
        let length = u32::from_be_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?)
            as usize;
        let kind = bytes.get(offset + 4..offset + 8)?;
        let end = offset.checked_add(8)?.checked_add(length)?;
        let data = bytes.get(offset + 8..end)?;
        let crc = u32::from_be_bytes(bytes.get(end..end.checked_add(4)?)?.try_into().ok()?);
        if kind == b"eXIf" {
            let mut hash = crc32fast::Hasher::new();
            hash.update(kind);
            hash.update(data);
            return (hash.finalize() == crc).then(|| data.to_vec());
        }
        if kind == b"IDAT" || kind == b"IEND" {
            return None;
        }
        offset = end.checked_add(4)?;
    }
}

pub(super) fn normalize_gif_header(bytes: &mut Vec<u8>) -> Result<(), ImagePreparationError> {
    let invalid = || {
        ImagePreparationError::Processing("unable to decode GIF: truncated header extension".into())
    };
    let packed = *bytes.get(10).ok_or_else(invalid)?;
    let mut read = 13;
    if packed & 0x80 != 0 {
        read += 3 * (1usize << ((packed & 7) + 1));
    }
    bytes.get(..read).ok_or_else(invalid)?;
    let mut write = read;
    while bytes.get(read) == Some(&0x21) {
        let start = read;
        let label = *bytes.get(read + 1).ok_or_else(invalid)?;
        read += 2;
        let empty_comment = label == 0xfe && bytes.get(read) == Some(&0);
        loop {
            let length = usize::from(*bytes.get(read).ok_or_else(invalid)?);
            read += 1;
            if length == 0 {
                break;
            }
            let end = read.checked_add(length).ok_or_else(invalid)?;
            bytes.get(read..end).ok_or_else(invalid)?;
            read = end;
        }
        if !empty_comment {
            if write != start {
                bytes.copy_within(start..read, write);
            }
            write += read - start;
        }
    }
    if write != read {
        let length = write + bytes.len() - read;
        bytes.copy_within(read.., write);
        bytes.truncate(length);
    }
    Ok(())
}

pub(super) fn gif_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let packed = *bytes.get(10)?;
    let mut offset = 13;
    if packed & 0x80 != 0 {
        offset += 3 * (1usize << ((packed & 7) + 1));
    }
    let mut profile = None;
    while bytes.get(offset) == Some(&0x21) {
        let label = *bytes.get(offset + 1)?;
        offset += 2;
        let first_size = usize::from(*bytes.get(offset)?);
        offset += 1;
        if first_size == 0 {
            continue;
        }
        let first = bytes.get(offset..offset.checked_add(first_size)?)?;
        offset += first_size;
        let is_icc = label == 0xff && first == b"ICCRGBG1012";
        if is_icc {
            profile = Some(Vec::new());
        }
        loop {
            let length = usize::from(*bytes.get(offset)?);
            offset += 1;
            if length == 0 {
                break;
            }
            let data = bytes.get(offset..offset.checked_add(length)?)?;
            offset += length;
            if is_icc {
                profile.as_mut()?.extend_from_slice(data);
            }
        }
    }
    profile
}

pub(super) fn encode_png(
    output: &mut Vec<u8>,
    pixels: &[u8],
    width: u32,
    height: u32,
    color: ColorType,
    icc: Option<Vec<u8>>,
    exif: Option<Vec<u8>>,
) -> Result<(), ImagePreparationError> {
    let mut info = png::Info::with_size(width, height);
    info.icc_profile = icc.filter(|data| !data.is_empty()).map(Cow::Owned);
    info.exif_metadata = exif.filter(|data| !data.is_empty()).map(Cow::Owned);
    let encode_error = |error: png::EncodingError| {
        ImagePreparationError::Processing(format!("unable to encode image as Png: {error}"))
    };
    let mut encoder = png::Encoder::with_info(output, info).map_err(encode_error)?;
    encoder.set_color(match color {
        ColorType::Rgb8 => png::ColorType::Rgb,
        ColorType::Rgba8 => png::ColorType::Rgba,
        _ => unreachable!("pixels are converted to RGB or RGBA"),
    });
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    encoder.set_filter(png::FilterType::Sub);
    encoder.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
    encoder
        .write_header()
        .map_err(encode_error)?
        .write_image_data(pixels)
        .map_err(encode_error)
}

pub(super) fn insert_jpeg_exif(
    bytes: &mut Vec<u8>,
    exif: &[u8],
) -> Result<(), ImagePreparationError> {
    if exif.is_empty() {
        return Ok(());
    }
    let length = exif.len().checked_add(2 + b"Exif\0\0".len()).and_then(|length| u16::try_from(length).ok()).ok_or_else(|| ImagePreparationError::Processing("unable to encode image as Jpeg: Exif metadata exceeds the JPEG APP1 segment length".into()))?;
    let mut segment = Vec::with_capacity(2 + usize::from(length));
    segment.extend_from_slice(&[0xff, 0xe1]);
    segment.extend_from_slice(&length.to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(exif);
    bytes.splice(2..2, segment);
    Ok(())
}
