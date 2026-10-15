use crate::bytes::Reader;

const AUDIO_SAMPLE_ENTRIES: [&[u8; 4]; 17] = [
    b"mp4a", b"alac", b"ac-3", b"ec-3", b"fLaC", b"Opus", b".mp3", b"lpcm", b"alaw", b"ulaw",
    b"raw ", b"sowt", b"twos", b"in24", b"in32", b"fl32", b"fl64",
];
const MDHD_UNKNOWN_DURATION_V0: u64 = u32::MAX as u64;

type Atom<'a> = ([u8; 4], &'a [u8]);

struct Track {
    id: u32,
    timescale: u64,
    media_duration: u64,
    sample_table_duration: u64,
}

pub fn duration_seconds(data: &[u8]) -> Option<f64> {
    let top = atoms(data)?;
    find(&top, b"ftyp")?;
    let moov = atoms(find(&top, b"moov")?)?;
    let mut track = None;
    for (_, trak) in moov.iter().filter(|(kind, _)| kind == b"trak") {
        if let Some(audio) = audio_track(trak)? {
            track = Some(audio);
            break;
        }
    }
    let track = track?;
    let Some(_) = find(&moov, b"mvex") else {
        return crate::ticks_to_seconds(track.sample_table_duration, 1, track.timescale);
    };
    let mut indexed: Option<(u64, u64)> = None;
    for (_, sidx) in top.iter().filter(|(kind, _)| kind == b"sidx") {
        if let Some((timescale, duration)) = sidx_duration(sidx, track.id)? {
            let total = match indexed {
                Some((indexed_timescale, _)) if indexed_timescale != timescale => return None,
                Some((_, total)) => total.checked_add(duration)?,
                None => duration,
            };
            indexed = Some((timescale, total));
        }
    }
    if let Some((timescale, ticks)) = indexed {
        return crate::ticks_to_seconds(ticks, 1, timescale);
    }
    if track.media_duration == MDHD_UNKNOWN_DURATION_V0 || track.media_duration == u64::MAX {
        return None;
    }
    crate::ticks_to_seconds(track.media_duration, 1, track.timescale)
}

fn atoms(data: &[u8]) -> Option<Vec<Atom<'_>>> {
    let mut reader = Reader::new(data);
    let mut atoms = Vec::new();
    while reader.remaining() > 0 {
        let size = reader.u32_be()?;
        let kind: [u8; 4] = reader.array()?;
        let body_len = match size {
            0 => reader.remaining(),
            1 => usize::try_from(reader.u64_be()?.checked_sub(16)?).ok()?,
            size => usize::try_from(size.checked_sub(8)?).ok()?,
        };
        if (kind == *b"mdat" || kind == *b"moof") && atoms.iter().any(|(kind, _)| kind == b"moov") {
            break;
        }
        atoms.push((kind, reader.bytes(body_len)?));
    }
    Some(atoms)
}

fn find<'a>(atoms: &[Atom<'a>], kind: &[u8; 4]) -> Option<&'a [u8]> {
    atoms
        .iter()
        .find(|(candidate, _)| candidate == kind)
        .map(|(_, body)| *body)
}

fn full_atom(body: &[u8]) -> Option<(u8, u32, Reader<'_>)> {
    let mut reader = Reader::new(body);
    let version_flags = reader.u32_be()?;
    let [version, ..] = version_flags.to_be_bytes();
    Some((version, version_flags & 0x00ff_ffff, reader))
}

fn audio_track(trak: &[u8]) -> Option<Option<Track>> {
    let trak = atoms(trak)?;
    let mdia = atoms(find(&trak, b"mdia")?)?;
    let stbl = atoms(find(&atoms(find(&mdia, b"minf")?)?, b"stbl")?)?;
    find(&stbl, b"stsz").or_else(|| find(&stbl, b"stz2"))?;
    find(&stbl, b"stsc")?;
    let (_, _, mut stsd) = full_atom(find(&stbl, b"stsd")?)?;
    if stsd.u32_be()? == 0 {
        return Some(None);
    }
    let entries = atoms(stsd.rest())?;
    let (entry, sample) = entries.first()?;
    if !AUDIO_SAMPLE_ENTRIES.contains(&entry) {
        return Some(None);
    }

    if sample.len() < 28 {
        return None;
    }
    let version = u16::from_be_bytes(sample[8..10].try_into().ok()?);
    let extension = match version {
        0 => 28,
        1 => 44,
        2 => 64,
        _ => return None,
    };
    atoms(sample.get(extension..)?)?;

    let (version, _, mut tkhd) = full_atom(find(&trak, b"tkhd")?)?;
    tkhd.skip(if version == 1 { 16 } else { 8 })?;
    let id = tkhd.u32_be()?;

    let (version, _, mut mdhd) = full_atom(find(&mdia, b"mdhd")?)?;
    let (timescale, media_duration) = if version == 1 {
        mdhd.skip(16)?;
        (mdhd.u32_be()?, mdhd.u64_be()?)
    } else {
        mdhd.skip(8)?;
        (mdhd.u32_be()?, u64::from(mdhd.u32_be()?))
    };
    if timescale == 0 {
        return None;
    }

    let (_, _, mut stts) = full_atom(find(&stbl, b"stts")?)?;
    let mut sample_table_duration = 0u64;
    for _ in 0..stts.u32_be()? {
        let count = u64::from(stts.u32_be()?);
        let delta = u64::from(stts.u32_be()?);
        sample_table_duration = sample_table_duration.checked_add(count.checked_mul(delta)?)?;
    }
    Some(Some(Track {
        id,
        timescale: u64::from(timescale),
        media_duration,
        sample_table_duration,
    }))
}

fn sidx_duration(sidx: &[u8], track_id: u32) -> Option<Option<(u64, u64)>> {
    let (version, _, mut sidx) = full_atom(sidx)?;
    if sidx.u32_be()? != track_id {
        return Some(None);
    }
    let timescale = sidx.u32_be()?;
    if timescale == 0 {
        return None;
    }
    sidx.skip(if version == 0 { 8 } else { 16 })?;
    sidx.skip(2)?;
    let mut total = 0u64;
    for _ in 0..sidx.u16_be()? {
        sidx.skip(4)?;
        total = total.checked_add(u64::from(sidx.u32_be()?))?;
        sidx.skip(4)?;
    }
    Some(Some((u64::from(timescale), total)))
}
