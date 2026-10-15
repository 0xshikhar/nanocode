use crate::{
    entropy::mem::{highbit32, read_u64},
    error::{Error, Result},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitDStreamStatus {
    Unfinished = 0,
    EndOfBuffer = 1,
    Completed = 2,
    Overflow = 3,
}
pub(crate) type BitContainer = u64;
pub(crate) const CONTAINER_BYTES: usize = core::mem::size_of::<BitContainer>();
pub(crate) const CONTAINER_BITS: u32 = BitContainer::BITS;
const BIT_MASK: [BitContainer; 32] = [
    0, 1, 3, 7, 0xF, 0x1F, 0x3F, 0x7F, 0xFF, 0x1FF, 0x3FF, 0x7FF, 0xFFF, 0x1FFF, 0x3FFF, 0x7FFF,
    0xFFFF, 0x1FFFF, 0x3FFFF, 0x7FFFF, 0xFFFFF, 0x1FFFFF, 0x3FFFFF, 0x7FFFFF, 0xFFFFFF, 0x1FFFFFF,
    0x3FFFFFF, 0x7FFFFFF, 0xFFFFFFF, 0x1FFFFFFF, 0x3FFFFFFF, 0x7FFFFFFF,
];
#[derive(Debug, Clone)]
pub(crate) struct BitDStream<'a> {
    pub(crate) bit_container: BitContainer,
    pub(crate) bits_consumed: u32,
    src: &'a [u8],
    pos: usize,
}
impl<'a> BitDStream<'a> {
    #[inline(always)]
    pub(crate) fn new(src: &'a [u8]) -> Result<Self> {
        if src.is_empty() {
            return Err(Error::SrcSizeWrong);
        }
        let last_byte = src[src.len() - 1];
        if last_byte == 0 {
            return Err(Error::Corruption("bitstream end marker is zero"));
        }
        let bits_consumed = 8 - highbit32(last_byte as u32);
        if src.len() >= CONTAINER_BYTES {
            let pos = src.len() - CONTAINER_BYTES;
            let bit_container = read_u64(src, src.len() - CONTAINER_BYTES);
            Ok(Self {
                bit_container,
                bits_consumed,
                src,
                pos,
            })
        } else {
            let mut bit_container: BitContainer = 0;
            for (index, byte) in src.iter().copied().enumerate() {
                bit_container |= BitContainer::from(byte) << (index * 8);
            }
            Ok(Self {
                bit_container,
                bits_consumed: bits_consumed + ((CONTAINER_BYTES - src.len()) * 8) as u32,
                src,
                pos: 0,
            })
        }
    }
    #[inline(always)]
    const fn reg_mask(&self) -> u32 {
        CONTAINER_BITS - 1
    }
    #[inline(always)]
    pub(crate) fn look_bits(&self, nb_bits: u32) -> BitContainer {
        let start = CONTAINER_BITS
            .wrapping_sub(self.bits_consumed)
            .wrapping_sub(nb_bits);
        self.get_middle_bits(start, nb_bits)
    }
    #[inline(always)]
    pub(crate) fn look_bits_fast(&self, nb_bits: u32) -> BitContainer {
        debug_assert!(nb_bits >= 1);
        let reg_mask = self.reg_mask();
        (self.bit_container << (self.bits_consumed & reg_mask))
            >> (((reg_mask + 1) - nb_bits) & reg_mask)
    }
    #[inline(always)]
    fn get_middle_bits(&self, start: u32, nb_bits: u32) -> BitContainer {
        let reg_mask = self.reg_mask();
        debug_assert!(nb_bits < BIT_MASK.len() as u32);
        (self.bit_container >> (start & reg_mask)) & BIT_MASK[nb_bits as usize]
    }
    #[inline(always)]
    pub(crate) const fn skip_bits(&mut self, nb_bits: u32) {
        self.bits_consumed += nb_bits;
    }
    #[inline(always)]
    pub(crate) const fn can_read_fast(&self, nb_bits: u32) -> bool {
        nb_bits != 0 && self.bits_consumed + nb_bits <= CONTAINER_BITS
    }
    #[inline(always)]
    pub(crate) fn read_bits(&mut self, nb_bits: u32) -> BitContainer {
        if nb_bits == 0 {
            return 0;
        }
        if self.can_read_fast(nb_bits) {
            return self.read_bits_fast(nb_bits);
        }
        let value = self.look_bits(nb_bits);
        self.skip_bits(nb_bits);
        value
    }
    #[inline(always)]
    pub(crate) fn read_bits_fast(&mut self, nb_bits: u32) -> BitContainer {
        let value = self.look_bits_fast(nb_bits);
        self.skip_bits(nb_bits);
        value
    }
    #[inline(always)]
    const fn bytes_behind_ptr(&self) -> usize {
        self.pos
    }
    #[inline(always)]
    pub(crate) fn reload_fast(&mut self) -> BitDStreamStatus {
        if self.bytes_behind_ptr() < CONTAINER_BYTES || self.bits_consumed > CONTAINER_BITS {
            return BitDStreamStatus::Overflow;
        }
        self.pos -= (self.bits_consumed >> 3) as usize;
        self.bits_consumed &= 7;
        self.bit_container = read_u64(self.src, self.pos);
        BitDStreamStatus::Unfinished
    }
    #[inline(always)]
    pub(crate) fn reload(&mut self) -> BitDStreamStatus {
        if self.bits_consumed > CONTAINER_BITS {
            return reload_overflow();
        }
        if self.bytes_behind_ptr() >= CONTAINER_BYTES {
            return self.reload_fast();
        }
        if self.pos == 0 {
            return reload_at_start(self.bits_consumed);
        }
        let mut nb_bytes = (self.bits_consumed >> 3) as usize;
        let mut result = BitDStreamStatus::Unfinished;
        let available = self.pos;
        if nb_bytes > available {
            nb_bytes = available;
            result = BitDStreamStatus::EndOfBuffer;
        }
        self.pos -= nb_bytes;
        self.bits_consumed -= (nb_bytes * 8) as u32;
        self.bit_container = read_u64(self.src, self.pos);
        result
    }
    #[inline(always)]
    pub(crate) const fn end_of_stream(&self) -> bool {
        self.pos == 0 && self.bits_consumed == CONTAINER_BITS
    }
}
#[cold]
#[inline(never)]
const fn reload_overflow() -> BitDStreamStatus {
    BitDStreamStatus::Overflow
}
#[cold]
#[inline(never)]
const fn reload_at_start(bits_consumed: u32) -> BitDStreamStatus {
    if bits_consumed < CONTAINER_BITS {
        BitDStreamStatus::EndOfBuffer
    } else {
        BitDStreamStatus::Completed
    }
}
