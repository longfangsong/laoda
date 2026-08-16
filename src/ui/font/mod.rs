//! 内嵌位图字体。文件名里的数字是生成时的**字号（px）**，不是 glyph box 高度
//! —— box 高度和 baseline 由 font-maker 按字体度量算出来，存在文件头里，排版
//! 请读 `header.height` / `header.baseline`，别写死。

use font_maker_core::format::Font;

static ASCII_18_BYTES: &[u8] = include_bytes!("PingFang_Regular_ASCII_18.bin");
static DIGIT_48_BYTES: &[u8] = include_bytes!("PingFang_Regular_DIGIT_48.bin");

pub fn ascii_18() -> Font<'static> {
    Font::new_fast(ASCII_18_BYTES).expect("invalid embedded font: ASCII_18")
}

pub fn digit_48() -> Font<'static> {
    Font::new_fast(DIGIT_48_BYTES).expect("invalid embedded font: DIGIT_48")
}
