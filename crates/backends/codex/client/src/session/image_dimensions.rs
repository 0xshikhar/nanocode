use std::io::Read;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, read::DecoderReader};

const MAX_IMAGE_HEADER_BYTES: usize = 256 * 1024;
const READ_CHUNK_BYTES: usize = 4 * 1024;

pub(crate) fn dimensions_from_base64(payload: &str) -> Option<(u32, u32)> {
    let mut decoder = DecoderReader::new(payload.as_bytes(), &BASE64_STANDARD);
    let mut header = Vec::with_capacity(READ_CHUNK_BYTES);
    let mut chunk = [0_u8; READ_CHUNK_BYTES];

    loop {
        if let Some(dimensions) = dimensions(&header) {
            return Some(dimensions);
        }
        let remaining = MAX_IMAGE_HEADER_BYTES.checked_sub(header.len())?;
        if remaining == 0 {
            return None;
        }
        let read = decoder
            .read(&mut chunk[..remaining.min(READ_CHUNK_BYTES)])
            .ok()?;
        if read == 0 {
            return dimensions(&header);
        }
        header.extend_from_slice(&chunk[..read]);
    }
}

fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    png_dimensions(bytes)
        .or_else(|| jpeg_dimensions(bytes))
        .or_else(|| gif_dimensions(bytes))
        .or_else(|| webp_dimensions(bytes))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    if bytes.get(..8)? != SIGNATURE
        || bytes.get(8..12)? != 13_u32.to_be_bytes()
        || bytes.get(12..16)? != b"IHDR"
    {
        return None;
    }
    nonzero_dimensions(be_u32(bytes, 16)?, be_u32(bytes, 20)?)
}

fn gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let signature = bytes.get(..6)?;
    if signature != b"GIF87a" && signature != b"GIF89a" {
        return None;
    }
    nonzero_dimensions(u32::from(le_u16(bytes, 6)?), u32::from(le_u16(bytes, 8)?))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..2)? != [0xff, 0xd8] {
        return None;
    }

    let mut offset = 2;
    loop {
        while bytes.get(offset).copied() == Some(0xff) {
            offset = offset.checked_add(1)?;
        }
        let marker = *bytes.get(offset)?;
        offset = offset.checked_add(1)?;

        match marker {
            0x00 | 0xd9 | 0xda => return None,
            0x01 | 0xd0..=0xd8 => continue,
            _ => {}
        }

        let segment_len = usize::from(be_u16(bytes, offset)?);
        if segment_len < 2 {
            return None;
        }
        let segment_end = offset.checked_add(segment_len)?;
        if segment_end > bytes.len() {
            return None;
        }
        if is_jpeg_start_of_frame(marker) {
            if segment_len < 8 {
                return None;
            }
            let height = u32::from(be_u16(bytes, offset.checked_add(3)?)?);
            let width = u32::from(be_u16(bytes, offset.checked_add(5)?)?);
            return nonzero_dimensions(width, height);
        }
        offset = segment_end;
    }
}

const fn is_jpeg_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
    )
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }

    let mut offset = 12_usize;
    loop {
        let chunk_type = bytes.get(offset..offset.checked_add(4)?)?;
        let chunk_len = usize::try_from(le_u32(bytes, offset.checked_add(4)?)?).ok()?;
        let data_offset = offset.checked_add(8)?;

        let found = match chunk_type {
            b"VP8X" => webp_extended_dimensions(bytes, data_offset, chunk_len),
            b"VP8 " => webp_lossy_dimensions(bytes, data_offset, chunk_len),
            b"VP8L" => webp_lossless_dimensions(bytes, data_offset, chunk_len),
            _ => None,
        };
        if found.is_some() {
            return found;
        }

        let padded_len = chunk_len.checked_add(chunk_len & 1)?;
        offset = data_offset.checked_add(padded_len)?;
        if offset > bytes.len() {
            return None;
        }
    }
}

fn webp_extended_dimensions(
    bytes: &[u8],
    data_offset: usize,
    chunk_len: usize,
) -> Option<(u32, u32)> {
    if chunk_len < 10 {
        return None;
    }
    let width = le_u24(bytes, data_offset.checked_add(4)?)?.checked_add(1)?;
    let height = le_u24(bytes, data_offset.checked_add(7)?)?.checked_add(1)?;
    nonzero_dimensions(width, height)
}

fn webp_lossy_dimensions(bytes: &[u8], data_offset: usize, chunk_len: usize) -> Option<(u32, u32)> {
    if chunk_len < 10
        || bytes.get(data_offset.checked_add(3)?..data_offset.checked_add(6)?)?
            != [0x9d, 0x01, 0x2a]
    {
        return None;
    }
    let width = u32::from(le_u16(bytes, data_offset.checked_add(6)?)? & 0x3fff);
    let height = u32::from(le_u16(bytes, data_offset.checked_add(8)?)? & 0x3fff);
    nonzero_dimensions(width, height)
}

fn webp_lossless_dimensions(
    bytes: &[u8],
    data_offset: usize,
    chunk_len: usize,
) -> Option<(u32, u32)> {
    if chunk_len < 5 || bytes.get(data_offset).copied()? != 0x2f {
        return None;
    }
    let bits = le_u32(bytes, data_offset.checked_add(1)?)?;
    let width = (bits & 0x3fff).checked_add(1)?;
    let height = ((bits >> 14) & 0x3fff).checked_add(1)?;
    nonzero_dimensions(width, height)
}

const fn nonzero_dimensions(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        None
    } else {
        Some((width, height))
    }
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn le_u24(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(3)?;
    let value = bytes.get(offset..end)?;
    Some(u32::from(value[0]) | (u32::from(value[1]) << 8) | (u32::from(value[2]) << 16))
}
