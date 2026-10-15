#[derive(Clone, Copy)]
pub struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub const fn remaining(&self) -> usize {
        self.data.len()
    }

    pub const fn rest(&self) -> &'a [u8] {
        self.data
    }

    pub fn bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.data.split_at_checked(len)?;
        self.data = tail;
        Some(head)
    }

    pub fn skip(&mut self, len: usize) -> Option<()> {
        self.bytes(len).map(drop)
    }

    pub fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N)?.try_into().ok()
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[byte]| byte)
    }

    pub fn u16_le(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }

    pub fn u32_le(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub fn u64_le(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }

    pub fn u16_be(&mut self) -> Option<u16> {
        self.array().map(u16::from_be_bytes)
    }

    pub fn u24_be(&mut self) -> Option<u32> {
        let [a, b, c] = self.array()?;
        Some(u32::from_be_bytes([0, a, b, c]))
    }

    pub fn u32_be(&mut self) -> Option<u32> {
        self.array().map(u32::from_be_bytes)
    }

    pub fn u64_be(&mut self) -> Option<u64> {
        self.array().map(u64::from_be_bytes)
    }
}
