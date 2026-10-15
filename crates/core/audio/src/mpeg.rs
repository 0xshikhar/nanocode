use crate::bytes::Reader;

const HEADER_LEN: usize = 4;
const DURATION_ESTIMATE_SAMPLE_FRAMES: u64 = 17;
const DURATION_ESTIMATE_SAMPLE_BYTES: usize = 16 * 1024;
const ID3V2_HEADER_LEN: usize = 10;
const ID3V2_FOOTER_FLAG: u8 = 0x10;
const VBRI_OFFSET: usize = HEADER_LEN + 32;
const VBRI_MIN_LEN: usize = 26;
const XING_MIN_LEN: usize = 8;
const LAME_EXTENSION_MIN_LEN: usize = 24;
const LAME_EXTENSION_CRC_LEN: usize = 12;
const LAME_DECODER_DELAY: u32 = 528 + 1;
const BITRATES_MPEG1_L1: [u32; 15] = [
    0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
];
const BITRATES_MPEG1_L2: [u32; 15] = [
    0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];
const BITRATES_MPEG1_L3: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const BITRATES_MPEG2_L1: [u32; 15] = [
    0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
];
const BITRATES_MPEG2_L23: [u32; 15] =
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Version {
    Mpeg1,
    Mpeg2,
    Mpeg25,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    One,
    Two,
    Three,
}

#[derive(Clone, Copy)]
struct Header {
    version: Version,
    layer: Layer,
    sample_rate: u32,
    mono: bool,
    has_crc: bool,
    frame_len: usize,
}

enum Tag {
    Xing { frames: Option<u32>, discard: u32 },
    Vbri { frames: u32 },
}

pub fn duration_seconds(data: &[u8]) -> Option<f64> {
    let start = skip_id3v2(data)?;
    let (first_pos, first) = first_frame(data, start)?;
    let first_frame = data.get(first_pos..first_pos.checked_add(first.frame_len)?)?;
    let tag = read_tag(first_frame, &first);
    let audio_start = if tag.is_some() {
        first_pos + first.frame_len
    } else {
        first_pos
    };
    let frames = || estimate_frames(data, audio_start);
    let samples_per_frame = first.samples_per_frame();
    let samples = match tag {
        Some(Tag::Xing {
            frames: Some(tagged),
            discard,
        }) => (u64::from(tagged) * samples_per_frame).saturating_sub(u64::from(discard)),
        Some(Tag::Vbri { frames: tagged }) => u64::from(tagged) * samples_per_frame,
        Some(Tag::Xing { frames: None, .. }) | None => frames()?.checked_mul(samples_per_frame)?,
    };
    crate::ticks_to_seconds(samples, 1, u64::from(first.sample_rate))
}

fn skip_id3v2(data: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    while let Some(header) = data.get(pos..pos + ID3V2_HEADER_LEN)
        && header.starts_with(b"ID3")
    {
        let size = header[6..10].iter().try_fold(0usize, |size, &byte| {
            (byte < 0x80).then_some(size << 7 | usize::from(byte))
        })?;
        let footer = if header[5] & ID3V2_FOOTER_FLAG != 0 {
            ID3V2_HEADER_LEN
        } else {
            0
        };
        pos = pos.checked_add(ID3V2_HEADER_LEN + size + footer)?;
        if pos > data.len() {
            return None;
        }
    }
    Some(pos)
}

fn first_frame(data: &[u8], start: usize) -> Option<(usize, Header)> {
    (start..data.len()).find_map(|pos| {
        let header = header_at(data, pos)?;
        let next = pos.checked_add(header.frame_len)?;
        if next > data.len() {
            return None;
        }
        match data.get(next..next + HEADER_LEN) {
            Some(_) => header_at(data, next)
                .filter(|candidate| candidate.is_similar(&header))
                .map(|_| (pos, header)),
            None => Some((pos, header)),
        }
    })
}

fn estimate_frames(data: &[u8], start: usize) -> Option<u64> {
    let mut position = start;
    let mut frames = 0u64;
    loop {
        let header = header_at(data, position)?;
        position = position.checked_add(header.frame_len)?;
        if position > data.len() {
            return None;
        }
        frames += 1;
        let bytes = position - start;
        if frames >= DURATION_ESTIMATE_SAMPLE_FRAMES || bytes > DURATION_ESTIMATE_SAMPLE_BYTES {
            let average = bytes as f64 / frames as f64;
            return Some(((data.len() - start) as f64 / average) as u64);
        }
    }
}

fn header_at(data: &[u8], pos: usize) -> Option<Header> {
    let word = data.get(pos..pos.checked_add(HEADER_LEN)?)?;
    Header::parse(u32::from_be_bytes(word.try_into().ok()?))
}

impl Header {
    fn parse(word: u32) -> Option<Self> {
        if word & 0xffe0_0000 != 0xffe0_0000 {
            return None;
        }
        let version = match (word >> 19) & 0b11 {
            0b00 => Version::Mpeg25,
            0b10 => Version::Mpeg2,
            0b11 => Version::Mpeg1,
            _ => return None,
        };
        let layer = match (word >> 17) & 0b11 {
            0b01 => Layer::Three,
            0b10 => Layer::Two,
            0b11 => Layer::One,
            _ => return None,
        };
        let bitrate_index = usize::try_from((word >> 12) & 0xf).ok()?;
        let bitrates = match (version, layer) {
            (Version::Mpeg1, Layer::One) => &BITRATES_MPEG1_L1,
            (Version::Mpeg1, Layer::Two) => &BITRATES_MPEG1_L2,
            (Version::Mpeg1, Layer::Three) => &BITRATES_MPEG1_L3,
            (_, Layer::One) => &BITRATES_MPEG2_L1,
            _ => &BITRATES_MPEG2_L23,
        };
        let bitrate = bitrates
            .get(bitrate_index)
            .copied()
            .filter(|&kbps| kbps != 0)?
            * 1000;
        let sample_rate = match ((word >> 10) & 0b11, version) {
            (0b00, Version::Mpeg1) => 44_100,
            (0b01, Version::Mpeg1) => 48_000,
            (0b10, Version::Mpeg1) => 32_000,
            (0b00, Version::Mpeg2) => 22_050,
            (0b01, Version::Mpeg2) => 24_000,
            (0b10, Version::Mpeg2) => 16_000,
            (0b00, Version::Mpeg25) => 11_025,
            (0b01, Version::Mpeg25) => 12_000,
            (0b10, Version::Mpeg25) => 8_000,
            _ => return None,
        };
        let mono = (word >> 6) & 0b11 == 0b11;
        if layer == Layer::Two {
            let invalid = if mono {
                matches!(bitrate, 224_000 | 256_000 | 320_000 | 384_000)
            } else {
                matches!(bitrate, 32_000 | 48_000 | 56_000 | 80_000)
            };
            if invalid {
                return None;
            }
        }
        let padding = (word >> 9) & 1;
        let frame_len = match layer {
            Layer::One => (12 * bitrate / sample_rate + padding) * 4,
            Layer::Two => 144 * bitrate / sample_rate + padding,
            Layer::Three if version == Version::Mpeg1 => 144 * bitrate / sample_rate + padding,
            Layer::Three => 72 * bitrate / sample_rate + padding,
        };
        Some(Self {
            version,
            layer,
            sample_rate,
            mono,
            has_crc: word & 0x1_0000 == 0,
            frame_len: usize::try_from(frame_len).ok()?,
        })
    }

    fn is_similar(&self, other: &Self) -> bool {
        self.version == other.version
            && self.layer == other.layer
            && self.sample_rate == other.sample_rate
            && self.mono == other.mono
    }

    const fn samples_per_frame(&self) -> u64 {
        match (self.layer, self.version) {
            (Layer::One, _) => 384,
            (Layer::Two, _) | (Layer::Three, Version::Mpeg1) => 1152,
            (Layer::Three, _) => 576,
        }
    }

    const fn side_info_len(&self) -> usize {
        match (self.version, self.mono) {
            (Version::Mpeg1, true) => 17,
            (Version::Mpeg1, false) => 32,
            (_, true) => 9,
            (_, false) => 17,
        }
    }

    const fn header_size(&self) -> usize {
        if self.has_crc {
            HEADER_LEN + 2
        } else {
            HEADER_LEN
        }
    }
}

fn read_tag(frame: &[u8], header: &Header) -> Option<Tag> {
    if header.layer != Layer::Three {
        return None;
    }
    read_xing(frame, header).or_else(|| read_vbri(frame, header))
}

fn read_xing(frame: &[u8], header: &Header) -> Option<Tag> {
    let offset = HEADER_LEN + header.side_info_len();
    let id = frame.get(offset..offset + 4)?;
    if frame.len() < offset + XING_MIN_LEN
        || (id != b"Xing" && id != b"Info")
        || frame
            .get(header.header_size()..offset)?
            .iter()
            .any(|&byte| byte != 0)
    {
        return None;
    }
    let mut reader = Reader::new(&frame[offset + 4..]);
    let flags = reader.u32_be()?;
    let frames = if flags & 0x1 != 0 {
        Some(reader.u32_be()?)
    } else {
        None
    };
    if flags & 0x2 != 0 {
        reader.skip(4)?;
    }
    if flags & 0x4 != 0 {
        reader.skip(100)?;
    }
    if flags & 0x8 != 0 {
        reader.skip(4)?;
    }
    let discard = if reader.remaining() >= LAME_EXTENSION_MIN_LEN {
        lame_discard(frame, header, reader).unwrap_or(0)
    } else {
        0
    };
    Some(Tag::Xing { frames, discard })
}

fn lame_discard(frame: &[u8], header: &Header, mut reader: Reader<'_>) -> Option<u32> {
    let encoder: [u8; 9] = reader.array()?;
    reader.skip(1 + 1 + 4 + 2 + 2 + 1 + 1)?;
    let trim = reader.u24_be()?;
    let (delay, padding) = if matches!(&encoder[..4], b"LAME" | b"Lavf" | b"Lavc") {
        (
            LAME_DECODER_DELAY + (trim >> 12),
            (trim & 0xfff).saturating_sub(LAME_DECODER_DELAY),
        )
    } else {
        (0, 0)
    };
    if reader.remaining() >= LAME_EXTENSION_CRC_LEN && (header.has_crc || &encoder[..4] == b"LAME")
    {
        reader.skip(1 + 1 + 2 + 4 + 2)?;
        let crc_pos = frame.len() - reader.remaining();
        let crc = reader.u16_be()?;
        if crc != 0 && crc != crc16_arc(&frame[..crc_pos]) {
            return None;
        }
    }
    Some(delay + padding)
}

fn read_vbri(frame: &[u8], header: &Header) -> Option<Tag> {
    if frame.len() < VBRI_OFFSET + VBRI_MIN_LEN
        || frame.get(VBRI_OFFSET..VBRI_OFFSET + 4)? != b"VBRI"
        || frame
            .get(header.header_size()..VBRI_OFFSET)?
            .iter()
            .any(|&byte| byte != 0)
    {
        return None;
    }
    let mut reader = Reader::new(&frame[VBRI_OFFSET + 4..]);
    if reader.u16_be()? != 1 {
        return None;
    }
    reader.skip(2 + 2 + 4)?;
    Some(Tag::Vbri {
        frames: reader.u32_be()?,
    })
}

fn crc16_arc(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |crc, &byte| {
        (0..8).fold(crc ^ u16::from(byte), |crc, _| {
            if crc & 1 != 0 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            }
        })
    })
}
