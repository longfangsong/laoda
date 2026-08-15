//! Claude 用量页面：3 组 {仪表盘 + 文字}，横向均分屏幕。
//!
//! ```text
//! ┌────────────────────────────────────────────┐
//! │    ╭───╮        ╭───╮        ╭───╮         │
//! │    │42%│        │77%│        │13%│         │
//! │    ╰───╯        ╰───╯        ╰───╯         │
//! │   SESSION        WEEK         OPUS         │
//! └────────────────────────────────────────────┘
//! ```

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb565,
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use font_consumer::FontTextStyle;

use crate::ui::{
    component::gauge::Gauge,
    font,
    page::{CHAR_SPACING, SCREEN_HEIGHT, SCREEN_WIDTH, truncate_to_width},
    theme,
};

/// 仪表盘组数
pub const GAUGE_COUNT: usize = 3;

const GAUGE_SIZE: usize = 90;
const GAUGE_BORDER: usize = 12;
const LABEL_HEIGHT: i32 = 18;
const GAUGE_TO_LABEL: i32 = 6;

/// {仪表盘 + 文字} 整体垂直居中
const BLOCK_HEIGHT: i32 = GAUGE_SIZE as i32 + GAUGE_TO_LABEL + LABEL_HEIGHT;
const BLOCK_TOP: i32 = (SCREEN_HEIGHT as i32 - BLOCK_HEIGHT) / 2;
const LABEL_TOP: i32 = BLOCK_TOP + GAUGE_SIZE as i32 + GAUGE_TO_LABEL;

/// 每组占据的横向宽度；除不尽的余量平分到两侧，保证左右留白对称
const CELL_WIDTH: i32 = SCREEN_WIDTH as i32 / GAUGE_COUNT as i32;
const CELLS_LEFT: i32 = (SCREEN_WIDTH as i32 - CELL_WIDTH * GAUGE_COUNT as i32) / 2;

type UsageGauge = Gauge<GAUGE_SIZE, GAUGE_BORDER>;

/// 一组用量：一个仪表盘 + 下方标签
#[derive(Clone, Copy)]
pub struct UsageItem {
    /// 标签，建议全大写、不含空格（见 [`crate::ui::page`] 的字体说明）
    pub label: &'static str,
    /// 用量比例，0.0..=1.0
    pub percentage: f32,
}

impl UsageItem {
    pub const fn new(label: &'static str, percentage: f32) -> Self {
        Self { label, percentage }
    }
}

pub struct ClaudeUsage {
    labels: [&'static str; GAUGE_COUNT],
    gauges: [UsageGauge; GAUGE_COUNT],
}

impl ClaudeUsage {
    pub fn new(items: [UsageItem; GAUGE_COUNT]) -> Self {
        Self {
            labels: core::array::from_fn(|i| items[i].label),
            gauges: core::array::from_fn(|i| UsageGauge::new(gauge_origin(i), items[i].percentage)),
        }
    }

    pub fn set_percentage(&mut self, index: usize, percentage: f32) {
        self.gauges[index].percentage(percentage);
    }

    pub fn set_label(&mut self, index: usize, label: &'static str) {
        self.labels[index] = label;
    }

    pub fn draw<D: DrawTarget<Color = Rgb565>>(&self, target: &mut D) -> Result<(), D::Error> {
        let area = target.bounding_box();
        target.fill_solid(&area, theme::BACKGROUND)?;

        let ascii = font::ascii_18();
        let label_style = FontTextStyle::new(&ascii, theme::TEXT_PRIMARY)
            .background_color(theme::BACKGROUND)
            .char_spacing(CHAR_SPACING);
        let centered = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Top)
            .build();

        for (i, label) in self.labels.iter().enumerate() {
            self.gauges[i].draw(target)?;
            Text::with_text_style(
                truncate_to_width(&ascii, label, CELL_WIDTH as u32),
                Point::new(cell_center_x(i), LABEL_TOP),
                label_style.clone(),
                centered,
            )
            .draw(target)?;
        }

        Ok(())
    }
}

const fn cell_center_x(index: usize) -> i32 {
    CELLS_LEFT + CELL_WIDTH * index as i32 + CELL_WIDTH / 2
}

const fn gauge_origin(index: usize) -> Point {
    Point::new(cell_center_x(index) - GAUGE_SIZE as i32 / 2, BLOCK_TOP)
}
