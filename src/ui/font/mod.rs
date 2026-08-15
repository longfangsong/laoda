use font_maker_core::format::Font;

static ASCII_18_BYTES: &[u8] = include_bytes!("PingFang_Regular_ASCII_18.bin");
static DIGIT_48_BYTES: &[u8] = include_bytes!("PingFang_Regular_DIGIT_48.bin");

pub fn ascii_18() -> Font<'static> {
    Font::new_fast(ASCII_18_BYTES).expect("invalid embedded font: ASCII_18")
}

pub fn digit_48() -> Font<'static> {
    Font::new_fast(DIGIT_48_BYTES).expect("invalid embedded font: DIGIT_48")
}
