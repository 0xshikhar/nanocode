#[inline(always)]
pub(crate) fn highbit32(value: u32) -> u32 {
    debug_assert!(value != 0);
    31u32 ^ value.leading_zeros()
}
#[inline(always)]
pub(crate) fn read_u64(input: &[u8], offset: usize) -> u64 {
    let bytes = input[offset..].first_chunk::<8>().unwrap();
    u64::from_le_bytes(*bytes)
}
