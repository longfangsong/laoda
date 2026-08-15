use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
};

pub struct CornerMask<const R: usize> {
    pub x: [u16; R],
}

const fn generate_corner_mask<const R: usize>() -> CornerMask<R> {
    const fn int_sqrt(n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut lo = 0u32;
        let mut hi = n;
        let mut result = 0u32;
        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            if mid * mid <= n {
                result = mid;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        result
    }

    let r = R as u32;
    let mut mask = [0u16; R];
    let mut i = 0usize;
    while i < R {
        // 圆心在(r,r)，圆弧从(r,0)到(0,r)，向外凸
        // row 0: inset = r (最窄, 矩形边缘)
        // row r: inset = 0 (最宽, 圆弧最外端)
        let y = i as u32;
        let arc = int_sqrt(2 * r * y - y * y);
        mask[i] = (r - arc) as u16;
        i += 1;
    }
    CornerMask { x: mask }
}

pub struct SplitRoundedRect<const R: usize> {
    origin: Point,
    size: Size,
    split_y: u16,
    top_color: Rgb565,
    bottom_color: Rgb565,
}

impl<const R: usize> SplitRoundedRect<R> {
    const MASK: CornerMask<R> = generate_corner_mask::<R>();

    pub fn new(origin: Point, size: Size) -> Self {
        Self {
            origin,
            size,
            split_y: 0,
            top_color: Rgb565::WHITE,
            bottom_color: Rgb565::BLACK,
        }
    }

    pub fn split_at(mut self, split: u16) -> Self {
        self.split_y = split.min(self.size.height as u16);
        self
    }

    pub fn colors(mut self, top: Rgb565, bottom: Rgb565) -> Self {
        self.top_color = top;
        self.bottom_color = bottom;
        self
    }

    pub fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let w = self.size.width as u16;
        let h = self.size.height as u16;
        if h == 0 || w == 0 {
            return Ok(());
        }

        let r = R as u16;

        let arc_start_row = r;
        let mut row = 0u16;

        while row < arc_start_row && row < h {
            let mask_val = Self::MASK.x[row as usize];
            let color = if row < self.split_y {
                self.top_color
            } else {
                self.bottom_color
            };
            Line::new(
                self.origin + Point::new(mask_val as i32, row as i32),
                self.origin + Point::new(w as i32 - 1 - mask_val as i32, row as i32),
            )
            .into_styled(PrimitiveStyle::with_stroke(color, 1))
            .draw(target)?;
            row += 1;
        }

        // === 中间矩形区域 ===
        let mid_end = if h > arc_start_row {
            h - arc_start_row
        } else {
            h
        };
        if row < mid_end {
            let split = self.split_y.clamp(row, mid_end);
            if split > row {
                target.fill_solid(
                    &Rectangle::new(
                        self.origin + Point::new(0, row as i32),
                        Size::new(w as u32, (split - row) as u32),
                    ),
                    self.top_color,
                )?;
            }
            if mid_end > split {
                target.fill_solid(
                    &Rectangle::new(
                        self.origin + Point::new(0, split as i32),
                        Size::new(w as u32, (mid_end - split) as u32),
                    ),
                    self.bottom_color,
                )?;
            }
            row = mid_end;
        }

        while row < h {
            let row_rel = (h - 1 - row) as usize;
            let inset = Self::MASK.x[row_rel.min(R - 1)];
            let color = if row < self.split_y {
                self.top_color
            } else {
                self.bottom_color
            };
            Line::new(
                self.origin + Point::new(inset as i32, row as i32),
                self.origin + Point::new(w as i32 - 1 - inset as i32, row as i32),
            )
            .into_styled(PrimitiveStyle::with_stroke(color, 1))
            .draw(target)?;
            row += 1;
        }

        Ok(())
    }
}
