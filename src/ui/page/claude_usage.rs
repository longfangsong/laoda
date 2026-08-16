//! Claude 用量页面：3 组 {仪表盘 + 标签}，横向均分屏幕。
//!
//! 纯渲染器（设计文档 §7.3）：数据从 [`UsageData`] 传入，页面无状态。
//! 标签写死在固件里，不走网络（PRD §4）。
//!
//! - **Unknown**（从未收到推送）：仪表显示 `--`，不显示 0%——0% 会被误读成"额度没用"
//! - **Stale**：仪表与标签用弱化配色（[`theme::TEXT_MUTED`]）
//! - **Fresh**：正常配色
//!
//! ```text
//! ┌────────────────────────────────────────────┐
//! │    ╭───╮        ╭───╮        ╭───╮         │
//! │    │42%│        │77%│        │ 13%│        │
//! │    ╰───╯        ╰───╯        ╰───╯         │
//! │   Session     Week      Fable             │
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

use crate::data::Freshness;
use crate::ui::{
    component::gauge::Gauge,
    font,
    page::{CHAR_SPACING, SCREEN_HEIGHT, SCREEN_WIDTH, truncate_to_width},
    theme,
};

/// 仪表盘组数
pub const GAUGE_COUNT: usize = 3;

/// 标签写死在固件里（PRD §4），顺序与推送包字段一致：session / week / fable
const LABELS: [&str; GAUGE_COUNT] = ["Session", "Week", "Fable"];

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

/// 页面数据：三个用量百分比 + 新鲜度
#[derive(Clone, Copy)]
pub struct UsageData {
    /// Session / Week / Fable 用量百分比 0..=100
    pub values: [u8; GAUGE_COUNT],
    pub freshness: Freshness,
}

pub struct ClaudeUsage;

impl ClaudeUsage {
    pub fn draw<D: DrawTarget<Color = Rgb565>>(
        target: &mut D,
        data: &UsageData,
    ) -> Result<(), D::Error> {
        target.fill_solid(&target.bounding_box(), theme::BACKGROUND)?;

        // Stale 时整组弱化（PRD §4）；Unknown 保持正常配色，只是数字为 `--`
        let muted = data.freshness == Freshness::Stale;
        let text_color = if muted {
            theme::TEXT_MUTED
        } else {
            theme::TEXT_PRIMARY
        };
        let fill_color = if muted {
            theme::TEXT_MUTED
        } else {
            theme::ACCENT
        };

        let ascii = font::ascii_18();
        let label_style = FontTextStyle::new(&ascii, text_color)
            .background_color(theme::BACKGROUND)
            .char_spacing(CHAR_SPACING);
        let centered = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Top)
            .build();

        // 仪表盘是值类型，每帧按数据构造（与 count_down 的进度条同模式）
        for (i, label) in LABELS.iter().enumerate() {
            let mut gauge = UsageGauge::new(gauge_origin(i), 0.0)
                .filled_part_color(fill_color)
                .text_color(text_color);
            match data.freshness {
                Freshness::Unknown => gauge = gauge.display("--"),
                _ => gauge = gauge.percentage(data.values[i] as f32 / 100.0),
            }
            gauge.draw(target)?;

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
