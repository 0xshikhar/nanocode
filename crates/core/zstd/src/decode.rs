use crate::{
    decode_out::DecodeOut,
    error::{Error, Result},
    frame::Format,
    literals::LiteralsState,
    sequence::{
        OutputLimit, RepeatOffsets, SequenceTablesState, TableTarget, decode_and_execute_sequences,
        parse_sequence_section,
    },
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderOptions {
    pub max_window_size: Option<u64>,
    pub max_output_size: Option<usize>,
    pub verify_checksum: bool,
    pub single_frame: bool,
    pub format: Format,
}
impl DecoderOptions {
    pub const DEFAULT_MAX_WINDOW_SIZE: u64 = 1 << 27;
}
impl Default for DecoderOptions {
    fn default() -> Self {
        Self {
            max_window_size: Some(Self::DEFAULT_MAX_WINDOW_SIZE),
            max_output_size: None,
            verify_checksum: true,
            single_frame: false,
            format: Format::Zstd1,
        }
    }
}
pub(crate) fn decode_compressed_block_into(
    src: &[u8],
    block_size_max: usize,
    frame_start: usize,
    window_size: usize,
    dictionary: Option<&[u8]>,
    literals_state: &mut LiteralsState,
    sequence_tables: &mut SequenceTablesState,
    repeat_offsets: &mut RepeatOffsets,
    literals_scratch: &mut Vec<u8>,
    out: &mut DecodeOut<'_>,
    limit: OutputLimit,
) -> Result<()> {
    let (literals_size, literals) = crate::literals::decode_literals_section_into(
        src,
        literals_state,
        block_size_max,
        literals_scratch,
    )?;
    if literals.len() > block_size_max {
        return Err(Error::Corruption(
            "compressed block literals exceed the frame block size limit",
        ));
    }
    let sequence_section = src.get(literals_size..).ok_or(Error::UnexpectedEof)?;
    let parsed_sequences =
        parse_sequence_section(sequence_section, sequence_tables, TableTarget::SequenceOnly)?;
    if parsed_sequences.number_of_sequences == 0
        && parsed_sequences.header_size != sequence_section.len()
    {
        return Err(Error::Corruption(
            "zero-sequence blocks must not contain additional sequence payload",
        ));
    }
    decode_and_execute_sequences(
        &parsed_sequences,
        sequence_tables,
        out,
        frame_start,
        window_size,
        block_size_max,
        dictionary,
        literals,
        repeat_offsets,
        limit,
    )
}
pub(crate) fn append_bytes(
    out: &mut DecodeOut<'_>,
    bytes: &[u8],
    limit: OutputLimit,
) -> Result<()> {
    let new_len = checked_output_len(out.len(), bytes.len())?;
    limit.check_total(new_len)?;
    out.append(bytes)
}
pub(crate) fn append_repeated(
    out: &mut DecodeOut<'_>,
    byte: u8,
    count: usize,
    limit: OutputLimit,
) -> Result<()> {
    let new_len = checked_output_len(out.len(), count)?;
    limit.check_total(new_len)?;
    out.append_repeated(byte, count)
}
fn checked_output_len(current: usize, add: usize) -> Result<usize> {
    current.checked_add(add).ok_or(Error::OutputSizeOverflow)
}
