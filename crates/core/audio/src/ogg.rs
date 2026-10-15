use crate::bytes::Reader;
use std::collections::BTreeMap;

const CRC_POLYNOMIAL: u32 = 0x04c1_1db7;
const CRC_TABLE: [u32; 256] = crc_table();

struct Stream {
    sample_rate: u32,
    duration: Option<u64>,
    ended: bool,
}

pub fn duration_seconds(data: &[u8]) -> Option<f64> {
    let mut reader = Reader::new(data);
    let mut streams = BTreeMap::new();
    let mut headers = true;
    while reader.remaining() > 0 {
        let rest = reader.rest();
        let Some(offset) = rest.windows(4).position(|window| window == b"OggS") else {
            break;
        };
        reader.skip(offset)?;
        let page_start = reader.rest();
        reader.skip(4)?;
        if reader.u8()? != 0 {
            return None;
        }
        let flags = reader.u8()?;
        let granule = reader.u64_le()?;
        let serial = reader.u32_le()?;
        reader.skip(4)?;
        let expected_crc = reader.u32_le()?;
        let segments = usize::from(reader.u8()?);
        let lacing = reader.bytes(segments)?;
        let payload_len: usize = lacing.iter().map(|&len| usize::from(len)).sum();
        let Some(payload) = reader.bytes(payload_len) else {
            break;
        };
        let page_len = page_start.len() - reader.remaining();
        if checksum(&page_start[..page_len]) != expected_crc {
            return None;
        }
        if flags & 2 != 0 {
            if !headers {
                break;
            }
            let packet_len = lacing.iter().take_while(|&&len| len == 255).count() * 255;
            let packet_len = packet_len + usize::from(*lacing.get(packet_len / 255)?);
            if let Some(stream) = identify(payload.get(..packet_len)?) {
                streams.insert(serial, stream);
            }
        } else {
            headers = false;
            if let Some(stream) = streams.get_mut(&serial)
                && !stream.ended
                && flags & 4 != 0
                && granule != u64::MAX
            {
                stream.duration = Some(granule);
                stream.ended = true;
            }
        }
    }
    let stream = streams.first_key_value()?.1;
    crate::ticks_to_seconds(stream.duration?, 1, u64::from(stream.sample_rate))
}

fn identify(packet: &[u8]) -> Option<Stream> {
    let (sample_rate, duration) = if packet.starts_with(b"OpusHead") {
        if packet.len() < 19 || packet[8] > 15 || packet[9] == 0 {
            return None;
        }
        match packet[18] {
            0 if packet[9] <= 2 => (),
            1 if packet[9] <= 8 && packet.len() >= 21 + usize::from(packet[9]) => (),
            _ => return None,
        }
        (48_000, None)
    } else if packet.starts_with(b"\x01vorbis") {
        let mut reader = Reader::new(packet);
        reader.skip(7)?;
        if reader.u32_le()? != 0 || reader.u8()? == 0 {
            return None;
        }
        let rate = reader.u32_le()?;
        reader.skip(12)?;
        let blocks = reader.u8()?;
        if !(6..=13).contains(&(blocks & 15))
            || !(6..=13).contains(&(blocks >> 4))
            || blocks & 15 > blocks >> 4
            || reader.u8()? & 1 == 0
        {
            return None;
        }
        (rate, None)
    } else if packet.starts_with(b"\x7fFLAC\x01\x00") {
        let mut reader = Reader::new(packet);
        reader.skip(9)?;
        if reader.array()? != *b"fLaC" || reader.u8()? & 0x7f != 0 || reader.u24_be()? != 34 {
            return None;
        }
        reader.skip(10)?;
        let info = reader.u64_be()?;
        reader.skip(16)?;
        let samples = info & 0x0000_000f_ffff_ffff;
        (
            u32::try_from(info >> 44).ok()?,
            (samples != 0).then_some(samples),
        )
    } else {
        return None;
    };
    (sample_rate != 0).then_some(Stream {
        sample_rate,
        duration,
        ended: false,
    })
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut crc = (index as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ CRC_POLYNOMIAL
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

fn checksum(page: &[u8]) -> u32 {
    page.iter().enumerate().fold(0u32, |crc, (index, &byte)| {
        let byte = if (22..26).contains(&index) { 0 } else { byte };
        let index = usize::from((crc >> 24) as u8 ^ byte);
        (crc << 8) ^ CRC_TABLE[index]
    })
}
