use core::convert::Infallible;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice as SharedSpiDevice;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::mutex::{Mutex, MutexGuard};
use embassy_time::Timer;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{Dimensions, OriginDimensions, Size},
    pixelcolor::{IntoStorage, Rgb565},
    primitives::{PointsIter, Rectangle},
};
use esp_hal::Async;
use esp_hal::gpio::Output;
use esp_hal::ledc::channel::ChannelIFace;
use esp_hal::ledc::{self, LowSpeed};
use esp_hal::spi::master::SpiDmaBus;

use crate::driver::st7789::St7789;

const WIDTH: u16 = 320;
const HEIGHT: u16 = 172;
const Y_OFFSET: u16 = 34;
const FRAMEBUFFER_SIZE: usize = WIDTH as usize * HEIGHT as usize * 2;
const REFRESH_INTERVAL_MS: u64 = 16;

pub type LcdSpi =
    SharedSpiDevice<'static, NoopRawMutex, SpiDmaBus<'static, Async>, Output<'static>>;

static FRAMEBUFFER: Mutex<CriticalSectionRawMutex, FrameBuffer> =
    Mutex::new(FrameBuffer([0; FRAMEBUFFER_SIZE]));

/// framebuffer 自上次刷新以来是否被写过。只在持有 FRAMEBUFFER 锁时读写，
/// 因此刷新任务不会漏掉与传输并发的绘制。初值为 true，保证首帧一定送出。
static DIRTY: AtomicBool = AtomicBool::new(true);

pub struct FrameBuffer([u8; FRAMEBUFFER_SIZE]);

pub struct Lcd<'d> {
    backlight: ledc::channel::Channel<'d, LowSpeed>,
}

impl<'d> Lcd<'d> {
    /// 创建 Lcd 并自动 spawn 刷新任务（~30 FPS）
    ///
    /// ```rust,ignore
    /// let mut lcd = Lcd::new(&spawner, spi_dev, dc_pin, rst_pin, backlight).await;
    /// let mut fb = lcd.frame().await;
    /// fb.clear(Rgb565::BLACK).unwrap();
    /// ```
    pub async fn new(
        spawner: &Spawner,
        spi: LcdSpi,
        dc: Output<'static>,
        rst: Output<'static>,
        backlight: ledc::channel::Channel<'d, LowSpeed>,
    ) -> Self {
        let st7789 = St7789::new(spi, dc, rst).await;
        spawner.spawn(refresh_task(st7789).unwrap());
        let mut result = Self { backlight };
        result.set_backlight(255);
        result
    }

    /// 获取 framebuffer 进行绘制，guard 存活期间刷新任务会等待，
    /// 因此绘制完成后应尽快 drop
    pub async fn frame(&mut self) -> MutexGuard<'static, CriticalSectionRawMutex, FrameBuffer> {
        FRAMEBUFFER.lock().await
    }

    pub fn set_backlight(&mut self, value: u8) {
        self.backlight.set_duty(value / 4).unwrap();
    }
}

#[embassy_executor::task]
async fn refresh_task(mut st7789: St7789<LcdSpi, Output<'static>, Output<'static>>) {
    loop {
        // 全屏一次传输（110KB @ 80MHz ≈ 11ms），期间持锁，绘制方等待。
        // 用 Timer::after 而不是 Ticker：单帧传输超过刷新间隔时 Ticker
        // 会一直"落后"而不让出执行权，刷新任务放锁后立刻重新抢锁，
        // 绘制方会被永远饿死。Timer::after 保证每帧后有让出窗口。
        let fb = FRAMEBUFFER.lock().await;
        if DIRTY.swap(false, Ordering::Relaxed) {
            st7789.set_window(0, Y_OFFSET, WIDTH, HEIGHT).await;
            st7789.write_pixels(&fb.0).await;
        }
        drop(fb);
        Timer::after_millis(REFRESH_INTERVAL_MS).await;
    }
}

impl FrameBuffer {
    fn set_pixel(&mut self, x: u16, y: u16, color: Rgb565) {
        if x < WIDTH && y < HEIGHT {
            let idx = (y as usize * WIDTH as usize + x as usize) * 2;
            self.0[idx..idx + 2].copy_from_slice(&color.into_storage().to_be_bytes());
        }
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for FrameBuffer {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        DIRTY.store(true, Ordering::Relaxed);
        for Pixel(point, color) in pixels {
            if point.x >= 0 && point.y >= 0 {
                self.set_pixel(point.x as u16, point.y as u16, color);
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        DIRTY.store(true, Ordering::Relaxed);
        for (point, color) in area.points().zip(colors) {
            if point.x >= 0 && point.y >= 0 {
                self.set_pixel(point.x as u16, point.y as u16, color);
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let drawable = area.intersection(&self.bounding_box());
        if drawable.size.width == 0 || drawable.size.height == 0 {
            return Ok(());
        }
        DIRTY.store(true, Ordering::Relaxed);
        let value = color.into_storage().to_be_bytes();
        let x0 = drawable.top_left.x as usize;
        let y0 = drawable.top_left.y as usize;
        let row_bytes = WIDTH as usize * 2;
        for y in y0..y0 + drawable.size.height as usize {
            let start = y * row_bytes + x0 * 2;
            let end = start + drawable.size.width as usize * 2;
            for pixel in self.0[start..end].chunks_exact_mut(2) {
                pixel.copy_from_slice(&value);
            }
        }
        Ok(())
    }
}
