//! 倒计时页面：左侧日历，右侧 2–4 组 {文字 + 进度条}，可见行整体垂直居中。
//!
//! 纯渲染器（设计文档 §7.3）：数据从 [`CountDownData`] 传入，页面无状态。
//! 条目模型与计算在 [`crate::data::countdown`]，host 侧可单测。
//!
//! ```text
//! ┌────────────────────────────────────────────┐
//! │            │  LABEL            123D        │
//! │  ┌──────┐  │  ▓▓▓▓▓▓▓▓░░░░░░░░░░░░░        │
//! │  │ AUG  │  │  LABEL             42H        │
//! │  │  14  │  │  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░        │
//! │  └──────┘  │  ...  x(2..4)                 │
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

use crate::data::countdown::{CountDownItem, ITEM_COUNT, ITEM_LABELS};
use crate::ui::{
    component::{calendar::Calendar, progress_bar::ProgressBar},
    font,
    page::{CHAR_SPACING, SCREEN_HEIGHT, SCREEN_WIDTH, text_width, truncate_to_width},
    theme,
};

/// 页面数据：未对时占位 vs 真实数据（条目 Copy，直接持有）
#[derive(Clone, Copy)]
pub enum CountDownData {
    /// NTP 未同步：日历与四行全部 `--` 占位，不套隐藏规则（PRD §3.5）
    Unknown,
    Ready {
        month: u8,
        day: u8,
        items: [CountDownItem; ITEM_COUNT],
    },
}

const MARGIN: i32 = 16;
/// 日历 103x103，在左侧垂直居中
const CALENDAR_ORIGIN: Point = Point::new(
    MARGIN,
    (SCREEN_HEIGHT as i32 - Calendar::HEIGHT_PX as i32) / 2,
);

/// 右侧栏起点：日历右边缘再留一段间距
const COLUMN_LEFT: i32 = MARGIN + Calendar::WIDTH_PX as i32 + 16;
const COLUMN_WIDTH: usize = (SCREEN_WIDTH as usize) - (COLUMN_LEFT as usize) - (MARGIN as usize);
const COLUMN_RIGHT: i32 = COLUMN_LEFT + COLUMN_WIDTH as i32;

/// 标签与行尾数值之间的最小间距；标签截断宽度按该行数值实测宽度推导
const LABEL_TO_DAYS: u32 = 6;

const LABEL_HEIGHT: i32 = 18;
const LABEL_TO_BAR: i32 = 2;
const BAR_HEIGHT: usize = 12;
/// 单行高度（文字 + 间隙 + 进度条 + 行距）
const ROW_HEIGHT: i32 = LABEL_HEIGHT + LABEL_TO_BAR + BAR_HEIGHT as i32 + 6;

/// 可见行整体垂直居中（设计文档 §7.2）
const fn rows_top(n: usize) -> i32 {
    (SCREEN_HEIGHT as i32 - ROW_HEIGHT * n as i32 + 6) / 2
}

type Bar = ProgressBar<COLUMN_WIDTH, BAR_HEIGHT>;

pub struct CountDown {
    calendar: Calendar,
}

impl CountDown {
    pub fn new() -> Self {
        Self {
            calendar: Calendar::new(CALENDAR_ORIGIN, 1, 1),
        }
    }

    pub fn draw<D: DrawTarget<Color = Rgb565>>(
        &mut self,
        target: &mut D,
        data: &CountDownData,
    ) -> Result<(), D::Error> {
        target.fill_solid(&target.bounding_box(), theme::BACKGROUND)?;

        // `None` = 未对时：日历画 `--`，四行固定占位
        let (month, day, items) = match data {
            CountDownData::Unknown => (0u8, 0u8, None),
            CountDownData::Ready { month, day, items } => (*month, *day, Some(items)),
        };
        self.calendar.set_date(month, day);
        self.calendar.draw(target)?;

        let n_rows = match items {
            None => ITEM_COUNT,
            // Year 与 SW Release 始终可见，Ready 模式 n_rows >= 2
            Some(items) => items.iter().filter(|it| it.visible()).count(),
        };
        let top = rows_top(n_rows);

        let ascii = font::ascii_18();
        let label_style = FontTextStyle::new(&ascii, theme::TEXT_PRIMARY)
            .background_color(theme::BACKGROUND)
            .char_spacing(CHAR_SPACING);
        let days_style = FontTextStyle::new(&ascii, theme::TEXT_SECONDARY)
            .background_color(theme::BACKGROUND)
            .char_spacing(CHAR_SPACING);
        let left_aligned = TextStyleBuilder::new()
            .alignment(Alignment::Left)
            .baseline(Baseline::Top)
            .build();
        let right_aligned = TextStyleBuilder::new()
            .alignment(Alignment::Right)
            .baseline(Baseline::Top)
            .build();

        // 隐藏项直接从序列中移除（设计文档 §3.4）；进度条按当前行位置每帧构造
        let mut row: i32 = 0;
        let mut value_buf = [0u8; 8];
        for i in 0..ITEM_COUNT {
            let (label, value, elapsed) = match items {
                None => (ITEM_LABELS[i], "--", 0.0f32),
                Some(items) => {
                    let item = &items[i];
                    if !item.visible() {
                        continue;
                    }
                    // "36000S"：数值按 unit 向上取整 + 单位字符
                    let len = crate::util::format_u16(&mut value_buf, item.value() as u16);
                    value_buf[len] = item.unit.suffix();
                    (
                        item.label,
                        core::str::from_utf8(&value_buf[..len + 1]).unwrap(),
                        item.elapsed(),
                    )
                }
            };
            let y = top + ROW_HEIGHT * row;
            row += 1;

            // 数值右对齐；标签截断上限按本行数值实测宽度推导，两者不撞
            let label_max = COLUMN_WIDTH as u32
                - text_width(&ascii, value).min(COLUMN_WIDTH as u32)
                - LABEL_TO_DAYS;
            Text::with_text_style(
                truncate_to_width(&ascii, label, label_max),
                Point::new(COLUMN_LEFT, y),
                label_style.clone(),
                left_aligned,
            )
            .draw(target)?;

            Text::with_text_style(
                value,
                Point::new(COLUMN_RIGHT, y),
                days_style.clone(),
                right_aligned,
            )
            .draw(target)?;

            Bar::new(
                Point::new(COLUMN_LEFT, y + LABEL_HEIGHT + LABEL_TO_BAR),
                elapsed,
            )
            .draw(target)?;
        }

        Ok(())
    }
}

impl Default for CountDown {
    fn default() -> Self {
        Self::new()
    }
}
