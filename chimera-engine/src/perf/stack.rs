pub const STACK_PAINT: u32 = 0xC0DE_C0DE;

pub fn untouched_words(words: impl IntoIterator<Item = u32>) -> usize {
    words.into_iter().take_while(|&w| w == STACK_PAINT).count()
}
