//! 整屏页面。一个 page 负责把若干 component 摆到 320x172 的屏幕上，
//! 并在每次 `draw` 时铺满背景，因此页面之间可以直接切换、无需手动清屏。
//!
//! 注意：ASCII_18 字体不含空格 glyph（覆盖 0x21..=0x7E），
//! 空格会被渲染成 0 宽度。页面里的文字标签请用单词或 `-` 连接。

pub mod claude_usage;
pub mod count_down;

pub use claude_usage::ClaudeUsage;
pub use count_down::CountDown;

/// 屏幕逻辑尺寸（Waveshare ESP32-C6-LCD-1.47 横屏）
pub const SCREEN_WIDTH: u32 = 320;
pub const SCREEN_HEIGHT: u32 = 172;

/// 字间距。页面上的文字都显式用这个值，好让排版计算和渲染一致
/// （与 `FontTextStyle` 的默认值相同）。
pub const CHAR_SPACING: u32 = 1;

/// 文字按字体渲染后的像素宽度（与 `FontTextStyle` 的渲染行为一致，无尾部间距）。
pub(crate) fn text_width(font: &font_maker_core::format::Font<'_>, text: &str) -> u32 {
    let mut width = 0u32;
    for c in text.chars() {
        let Some(entry) = font.get_glyph_entry(c as u32) else {
            continue;
        };
        width += entry.width as u32 + if width == 0 { 0 } else { CHAR_SPACING };
    }
    width
}

/// 按像素宽度截断文字，返回能放进 `max_width` 的最长前缀。
/// 字体里没有的字符（例如空格）宽度按 0 计，与 `FontTextStyle` 的渲染行为一致。
pub(crate) fn truncate_to_width<'a>(
    font: &font_maker_core::format::Font<'_>,
    text: &'a str,
    max_width: u32,
) -> &'a str {
    let mut width = 0u32;
    for (idx, c) in text.char_indices() {
        let Some(entry) = font.get_glyph_entry(c as u32) else {
            continue;
        };
        let advance = entry.width as u32 + if width == 0 { 0 } else { CHAR_SPACING };
        if width + advance > max_width {
            return &text[..idx];
        }
        width += advance;
    }
    text
}
