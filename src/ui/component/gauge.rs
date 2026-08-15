use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{AngleUnit, Point, Size},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle, Sector},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use font_consumer::FontTextStyle;

use crate::ui::{font, theme};

// 字体格式暂无 baseline 元数据，Baseline::Middle 按 glyph 全高居中，
// 而 ASCII_18 的数字/百分号墨迹只占 0..=14 / 18 行，视觉重心偏上 2px，手动补偿。
// 待 font-maker 格式 v2 加入 baseline 后移除（见其 docs/backlog.md）。
const TEXT_Y_NUDGE: i32 = 2;

pub struct Gauge<const SIZE: usize = 93, const BORDER: usize = 13> {
    top_left: Point,
    percentage: f32,
    filled_part_color: Rgb565,
    empty_part_color: Rgb565,
    background_color: Rgb565,
    text_color: Rgb565,
}

impl<const SIZE: usize, const BORDER: usize> Gauge<SIZE, BORDER> {
    pub fn new(top_left: Point, percentage: f32) -> Self {
        Self {
            top_left,
            percentage,
            filled_part_color: theme::ACCENT,
            empty_part_color: theme::TRACK,
            background_color: theme::BACKGROUND,
            text_color: theme::TEXT_PRIMARY,
        }
    }

    pub fn filled_part_color(mut self, color: Rgb565) -> Self {
        self.filled_part_color = color;
        self
    }

    pub fn empty_part_color(mut self, color: Rgb565) -> Self {
        self.empty_part_color = color;
        self
    }

    pub fn background_color(mut self, color: Rgb565) -> Self {
        self.background_color = color;
        self
    }

    pub fn text_color(mut self, color: Rgb565) -> Self {
        self.text_color = color;
        self
    }

    pub fn percentage(&mut self, percentage: f32) {
        self.percentage = percentage;
    }

    pub fn draw<D: DrawTarget<Color = Rgb565>>(&self, target: &mut D) -> Result<(), D::Error> {
        let percentage = self.percentage.clamp(0.0, 1.0);
        let diameter = SIZE as u32;

        Circle::new(self.top_left, diameter)
            .into_styled(PrimitiveStyle::with_fill(self.empty_part_color))
            .draw(target)?;
        if percentage > 0.0 {
            // 从 12 点方向顺时针填充：embedded-graphics 的角度以 3 点方向为 0°、
            // 逆时针为正，所以起点取 90°、扫过负角度。
            Sector::new(
                self.top_left,
                diameter,
                90.0.deg(),
                (-percentage * 360.0).deg(),
            )
            .into_styled(PrimitiveStyle::with_fill(self.filled_part_color))
            .draw(target)?;
        }

        let inner_diameter = diameter - 2 * BORDER as u32;
        let offset = BORDER as i32;
        let inner_circle = Circle::new(self.top_left + Point::new(offset, offset), inner_diameter);
        let circle_center = inner_circle.center();
        inner_circle
            .into_styled(PrimitiveStyle::with_fill(self.background_color))
            .draw(target)?;

        let ascii = font::ascii_18();
        let char_style =
            FontTextStyle::new(&ascii, self.text_color).background_color(self.background_color);
        let text_style = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Middle)
            .build();

        let pct = (percentage * 100.0 + 0.5) as u8;
        let mut buf = [0u8; 4];
        let nlen = crate::util::format_u8(&mut buf, pct);
        buf[nlen] = b'%';
        let text = core::str::from_utf8(&buf[..nlen + 1]).unwrap();
        Text::with_text_style(
            text,
            circle_center + Point::new(0, TEXT_Y_NUDGE),
            char_style,
            text_style,
        )
        .draw(target)?;

        Ok(())
    }
}

impl<const SIZE: usize, const BORDER: usize> Dimensions for Gauge<SIZE, BORDER> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(self.top_left, Size::new(SIZE as u32, SIZE as u32))
    }
}
