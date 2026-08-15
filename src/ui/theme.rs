//! 全局配色。所有组件/页面的默认颜色都取自这里，避免同一个色值散落在多处。

use embedded_graphics::pixelcolor::Rgb565;

/// 把 8-8-8 的 sRGB 分量压成 Rgb565，方便直接抄设计稿里的 hex
const fn rgb565(r: u8, g: u8, b: u8) -> Rgb565 {
    Rgb565::new(r >> 3, g >> 2, b >> 3)
}

/// 页面底色 (#0D0E10)
pub const BACKGROUND: Rgb565 = rgb565(13, 14, 16);

/// 主强调色，进度条/仪表盘的已完成部分 (#C5785B)
pub const ACCENT: Rgb565 = rgb565(197, 120, 91);

/// 进度条/仪表盘的未完成部分 (#1F2023)
pub const TRACK: Rgb565 = rgb565(31, 32, 35);

/// 正文文字 (#EBEBEB)
pub const TEXT_PRIMARY: Rgb565 = rgb565(235, 235, 235);

/// 次要文字，比正文暗一档 (#8C8C8C)
pub const TEXT_SECONDARY: Rgb565 = rgb565(140, 140, 140);

/// 日历顶栏的苹果日历红 (#E06152)
pub const CALENDAR_BAND: Rgb565 = rgb565(224, 97, 82);

/// 日历纸面 (#F5F5F5)
pub const CALENDAR_BODY: Rgb565 = rgb565(245, 245, 245);

/// 日历上的日期数字 (#2C2C2C)
pub const CALENDAR_DAY: Rgb565 = rgb565(44, 44, 44);
