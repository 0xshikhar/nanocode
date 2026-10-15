use crate::{
    error::{Error, Result},
    frame::read_u24_le,
};
pub const BLOCK_SIZE_MAX: usize = 128 * 1024;
pub const BLOCK_HEADER_SIZE: usize = 3;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Raw,
    Rle,
    Compressed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockHeader {
    pub last_block: bool,
    pub block_type: BlockType,
    pub block_size: u32,
}
impl BlockHeader {
    pub const SIZE: usize = BLOCK_HEADER_SIZE;
    pub fn parse(src: &[u8]) -> Result<Self> {
        if src.len() < BLOCK_HEADER_SIZE {
            return Err(Error::UnexpectedEof);
        }
        let value = read_u24_le(&src[..BLOCK_HEADER_SIZE]);
        let last_block = (value & 1) != 0;
        let block_type = match (value >> 1) & 0x3 {
            0 => BlockType::Raw,
            1 => BlockType::Rle,
            2 => BlockType::Compressed,
            _ => return Err(Error::Corruption("reserved block type")),
        };
        let block_size = value >> 3;
        Ok(Self {
            last_block,
            block_type,
            block_size,
        })
    }
    pub(crate) const fn payload_size(self) -> usize {
        match self.block_type {
            BlockType::Raw | BlockType::Compressed => self.block_size as usize,
            BlockType::Rle => 1,
        }
    }
}
