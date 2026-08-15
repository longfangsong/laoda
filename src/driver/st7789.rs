use embassy_time::Timer;
use embedded_hal_async::spi::SpiDevice;

pub struct St7789<SPI, DC, RST> {
    spi: SPI,
    dc: DC,
    reset: RST,
}

impl<SPI, DC, RST> St7789<SPI, DC, RST>
where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
{
    pub async fn new(spi: SPI, dc: DC, reset: RST) -> Self {
        let mut result = Self { spi, dc, reset };
        result.hardware_reset().await;
        result.init_registers().await;
        result.display_on().await;
        result
    }

    fn set_dc(&mut self, high: bool) {
        if high {
            self.dc.set_high().unwrap();
        } else {
            self.dc.set_low().unwrap();
        }
    }

    pub async fn send_command(&mut self, command: u8) {
        self.set_dc(false);
        self.spi.write(&[command]).await.unwrap();
    }

    pub async fn send_data(&mut self, data: &[u8]) {
        self.set_dc(true);
        self.spi.write(data).await.unwrap();
    }

    pub async fn send_command_with_data(&mut self, command: u8, data: &[u8]) {
        self.send_command(command).await;
        if !data.is_empty() {
            self.send_data(data).await;
        }
    }

    async fn hardware_reset(&mut self) {
        self.reset.set_low().unwrap();
        Timer::after_millis(10).await;
        self.reset.set_high().unwrap();
        Timer::after_millis(10).await;
    }

    async fn init_registers(&mut self) {
        self.send_command(0x11).await; // Sleep Out
        Timer::after_millis(100).await;
        // Memory Data Access Control: MV (landscape) + BGR.
        // The panel's color filter is physically B-G-R ordered; declaring BGR
        // here lets the controller map incoming RGB565 to the right subpixels.
        self.send_command_with_data(0x36, &[0x28]).await;
        self.send_command_with_data(0x3A, &[0x55]).await; // Pixel Format Set (RGB565)
        self.send_command_with_data(0xB0, &[0x00, 0xF0]).await; // RAM Control
        self.send_command_with_data(0xB2, &[0x0C, 0x0C, 0x00, 0x33, 0x33])
            .await; // Porch Setting
        self.send_command_with_data(0xB7, &[0x75]).await; // Gate Control
        self.send_command_with_data(0xBB, &[0x1A]).await; // VCOM Setting
        self.send_command_with_data(0xC0, &[0x80]).await; // LCM Control
        self.send_command_with_data(0xC2, &[0x01, 0xFF]).await; // VCOM Register Set
        self.send_command_with_data(0xC3, &[0x13]).await; // Gate Equalization Time
        self.send_command_with_data(0xC4, &[0x20]).await; // Gate Bias
        self.send_command_with_data(0xD0, &[0xA4, 0xA1]).await; // Power Control 1
        self.send_command_with_data(
            0xE0,
            &[
                0xD0, 0x0D, 0x14, 0x0D, 0x0D, 0x09, 0x38, 0x44, 0x4E, 0x3A, 0x17, 0x18, 0x2F, 0x30,
            ],
        )
        .await; // PGAMCTRL (Positive Gamma)
        self.send_command_with_data(
            0xE1,
            &[
                0xD0, 0x09, 0x0F, 0x08, 0x07, 0x14, 0x37, 0x44, 0x4D, 0x38, 0x15, 0x16, 0x2C, 0x2E,
            ],
        )
        .await; // NGAMCTRL (Negative Gamma)
        // FRCTRL2 (Frame Rate Control): 0x01 = 111Hz panel scan.
        // No TE pin is routed on this board, so tearing cannot be eliminated;
        // a faster panel scan shortens how long the tear boundary stays visible.
        self.send_command_with_data(0xC6, &[0x01]).await;
        self.send_command_with_data(0x21, &[]).await; // Display Inversion On
    }

    // 预留：屏保/休眠时关灯。目前没有调用方。
    #[allow(dead_code)]
    async fn display_off(&mut self) {
        self.send_command(0x28).await;
    }

    async fn display_on(&mut self) {
        self.send_command(0x29).await;
    }

    pub async fn set_window(&mut self, x: u16, y: u16, width: u16, height: u16) {
        let x0 = x;
        let x1 = x0 + width - 1;
        let y0 = y;
        let y1 = y + height - 1;

        // CASET
        self.send_command_with_data(
            0x2A,
            &[(x0 >> 8) as u8, x0 as u8, (x1 >> 8) as u8, x1 as u8],
        )
        .await;
        // RASET
        self.send_command_with_data(
            0x2B,
            &[(y0 >> 8) as u8, y0 as u8, (y1 >> 8) as u8, y1 as u8],
        )
        .await;
    }

    pub async fn write_pixels(&mut self, pixels: &[u8]) {
        self.send_command(0x2C).await; // RAMWR
        self.send_data(pixels).await;
    }
}
