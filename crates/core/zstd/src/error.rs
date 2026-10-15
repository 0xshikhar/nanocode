use core::fmt;
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    Generic,
    InvalidParameter(&'static str),
    DstSizeTooSmall,
    SrcSizeWrong,
    UnexpectedEof,
    BadMagic(u32),
    Corruption(&'static str),
    TableLogTooLarge,
    MaxSymbolValueTooLarge,
    MaxSymbolValueTooSmall,
    WorkSpaceTooSmall,
    WindowSizeTooLarge {
        window_size: u64,
        max_window_size: u64,
    },
    DictionaryRequired(Option<u32>),
    DictionaryMismatch {
        expected: u32,
        actual: u32,
    },
    OutputSizeTooLarge {
        output_size: u64,
        max_output_size: usize,
    },
    ContentSizeTooLarge(u64),
    OutputSizeOverflow,
    ContentSizeMismatch {
        expected: u64,
        actual: u64,
    },
    TrailingInput {
        offset: usize,
    },
    ChecksumMismatch {
        expected: u32,
        actual: u32,
    },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generic => f.write_str("generic error"),
            Self::InvalidParameter(parameter) => {
                write!(f, "invalid parameter: {parameter}")
            }
            Self::DstSizeTooSmall => f.write_str("destination buffer too small"),
            Self::SrcSizeWrong => f.write_str("source size is invalid"),
            Self::UnexpectedEof => f.write_str("unexpected end of input"),
            Self::BadMagic(magic) => {
                write!(f, "invalid frame magic number 0x{magic:08x}")
            }
            Self::Corruption(message) => write!(f, "corruption detected: {message}"),
            Self::TableLogTooLarge => f.write_str("table log too large"),
            Self::MaxSymbolValueTooLarge => f.write_str("maximum symbol value too large"),
            Self::MaxSymbolValueTooSmall => f.write_str("maximum symbol value too small"),
            Self::WorkSpaceTooSmall => f.write_str("workspace too small"),
            Self::WindowSizeTooLarge {
                window_size,
                max_window_size,
            } => {
                write!(
                    f,
                    "frame window size {window_size} exceeds configured limit {max_window_size}"
                )
            }
            Self::DictionaryRequired(Some(dictionary_id)) => {
                write!(f, "frame requires dictionary id {dictionary_id}")
            }
            Self::DictionaryRequired(None) => f.write_str("frame requires a dictionary"),
            Self::DictionaryMismatch { expected, actual } => {
                write!(
                    f,
                    "dictionary mismatch: frame requires id {expected}, got {actual}"
                )
            }
            Self::OutputSizeTooLarge {
                output_size,
                max_output_size,
            } => {
                write!(
                    f,
                    "decoded output size {output_size} exceeds configured limit {max_output_size}"
                )
            }
            Self::ContentSizeTooLarge(content_size) => {
                write!(f, "frame content size {content_size} exceeds this platform")
            }
            Self::OutputSizeOverflow => f.write_str("decoded output size overflowed this platform"),
            Self::ContentSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "frame content size mismatch: expected {expected}, got {actual}"
                )
            }
            Self::TrailingInput { offset } => {
                write!(
                    f,
                    "input is not a single Zstandard frame: unexpected input at byte {offset}"
                )
            }
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "content checksum mismatch: expected 0x{expected:08x}, got 0x{actual:08x}"
                )
            }
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = core::result::Result<T, Error>;
