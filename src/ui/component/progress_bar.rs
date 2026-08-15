use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};

use crate::ui::theme;

pub struct ProgressBar<const W: usize = 93, const H: usize = 13> {
    top_left: Point,
    percentage: f32,
    filled_part_color: Rgb565,
    empty_part_color: Rgb565,
}

impl<const W: usize, const H: usize> ProgressBar<W, H> {
    pub fn new(top_left: Point, percentage: f32) -> Self {
        Self {
            top_left,
            percentage,
            filled_part_color: theme::ACCENT,
            empty_part_color: theme::TRACK,
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

    pub fn percentage(&mut self, percentage: f32) {
        self.percentage = percentage;
    }

    pub fn draw<D: DrawTarget<Color = Rgb565>>(&self, target: &mut D) -> Result<(), D::Error> {
        let percentage = self.percentage.clamp(0.0, 1.0);

        Rectangle::new(self.top_left, Size::new(W as u32, H as u32))
            .into_styled(PrimitiveStyle::with_fill(self.empty_part_color))
            .draw(target)?;

        let filled_width = (W as f32 * percentage + 0.5) as u32;
        if filled_width > 0 {
            Rectangle::new(self.top_left, Size::new(filled_width, H as u32))
                .into_styled(PrimitiveStyle::with_fill(self.filled_part_color))
                .draw(target)?;
        }

        Ok(())
    }
}

impl<const W: usize, const H: usize> Dimensions for ProgressBar<W, H> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(self.top_left, Size::new(W as u32, H as u32))
    }
}
