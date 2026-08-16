use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::{Rgb565, RgbColor},
    prelude::*,
    primitives::Rectangle,
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use font_consumer::FontTextStyle;

use crate::ui::{font, split_rounded_rect::SplitRoundedRect, theme};

const WIDTH: u32 = 103;
const HEIGHT: u32 = 103;
const RADIUS: usize = 16;
const BAND_HEIGHT: u16 = 32;

const MONTH_NAMES: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

pub struct Calendar {
    top_left: Point,
    month: u8,
    day: u8,
    band_color: Rgb565,
    body_color: Rgb565,
    month_color: Rgb565,
    day_color: Rgb565,
}

impl Calendar {
    /// 组件外形尺寸，供页面排版使用
    pub const WIDTH_PX: u32 = WIDTH;
    pub const HEIGHT_PX: u32 = HEIGHT;

    pub fn new(top_left: Point, month: u8, day: u8) -> Self {
        Self {
            top_left,
            month: month.clamp(1, 12),
            day: day.clamp(1, 31),
            band_color: theme::CALENDAR_BAND,
            body_color: theme::CALENDAR_BODY,
            month_color: Rgb565::WHITE,
            day_color: theme::CALENDAR_DAY,
        }
    }

    /// `month == 0` 表示尚未对时，画 `--` 占位（PRD §3.5）
    pub fn set_date(&mut self, month: u8, day: u8) {
        self.month = month.clamp(0, 12);
        self.day = day.clamp(0, 31);
    }

    pub fn draw<D: DrawTarget<Color = Rgb565>>(&self, target: &mut D) -> Result<(), D::Error> {
        SplitRoundedRect::<RADIUS>::new(self.top_left, Size::new(WIDTH, HEIGHT))
            .split_at(BAND_HEIGHT)
            .colors(self.band_color, self.body_color)
            .draw(target)?;

        let centered = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Middle)
            .build();

        let ascii_font = font::ascii_18();
        let month_style =
            FontTextStyle::new(&ascii_font, self.month_color).background_color(self.band_color);
        let month_center = self.top_left + Point::new(WIDTH as i32 / 2, BAND_HEIGHT as i32 / 2);
        let day_center =
            self.top_left + Point::new(WIDTH as i32 / 2, (BAND_HEIGHT as i32 + HEIGHT as i32) / 2);

        // 尚未对时：色带与主体都画 `--`（ASCII_18 有连字符字形，digit_48 没有）
        if self.month == 0 || self.day == 0 {
            Text::with_text_style("--", month_center, month_style, centered).draw(target)?;
            let placeholder_style =
                FontTextStyle::new(&ascii_font, self.day_color).background_color(self.body_color);
            Text::with_text_style("--", day_center, placeholder_style, centered).draw(target)?;
            return Ok(());
        }

        Text::with_text_style(
            MONTH_NAMES[(self.month - 1) as usize],
            month_center,
            month_style,
            centered,
        )
        .draw(target)?;

        let digit_font = font::digit_48();
        let day_style =
            FontTextStyle::new(&digit_font, self.day_color).background_color(self.body_color);
        let mut buf = [0u8; 3];
        let len = crate::util::format_u8(&mut buf, self.day);
        let day_text = core::str::from_utf8(&buf[..len]).unwrap();
        Text::with_text_style(day_text, day_center, day_style, centered).draw(target)?;

        Ok(())
    }
}

impl Dimensions for Calendar {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(self.top_left, Size::new(WIDTH, HEIGHT))
    }
}
