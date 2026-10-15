#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GeneralCategory {
    Other,
    Letter,
    NonspacingMark,
    SpacingMark,
    EnclosingMark,
    DecimalNumber,
}

const PAGE_BITS: u32 = 7;
const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_INDEX: &[u8] = include_bytes!("unicode-index.bin");
const CATEGORIES: &[u8] = include_bytes!("unicode-categories.bin");

pub(crate) fn get_general_category(value: char) -> GeneralCategory {
    let codepoint = value as usize;
    let Some(&page) = PAGE_INDEX.get(codepoint >> PAGE_BITS) else {
        return GeneralCategory::Other;
    };
    match CATEGORIES[usize::from(page) * PAGE_SIZE + (codepoint & (PAGE_SIZE - 1))] {
        0 => GeneralCategory::Other,
        1 => GeneralCategory::Letter,
        2 => GeneralCategory::NonspacingMark,
        3 => GeneralCategory::SpacingMark,
        4 => GeneralCategory::EnclosingMark,
        5 => GeneralCategory::DecimalNumber,
        category => unreachable!("invalid embedded Unicode category {category}"),
    }
}
