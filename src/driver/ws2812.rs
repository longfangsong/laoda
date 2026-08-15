use embedded_graphics::pixelcolor::{Rgb888, RgbColor};
use esp_hal::gpio::Level;
use esp_hal::rmt::{Channel, PulseCode, Rmt, Tx, TxChannelConfig, TxChannelCreator};
use esp_hal::time::Rate;

pub struct Ws2812 {
    channel: Channel<'static, esp_hal::Async, Tx>,
}

// Use the ESP32-C6 RMT's 80 MHz source clock with a channel divider of 7,
// giving the same 10 MHz resolution as the manufacturer's driver. These
// values match its led_strip encoder exactly: T0H/T0L = 0.3/0.9 us and
// T1H/T1L = 0.9/0.3 us.
const BIT0: PulseCode = PulseCode::new(Level::High, 3, Level::Low, 9);
const BIT1: PulseCode = PulseCode::new(Level::High, 9, Level::Low, 3);
// The manufacturer's driver uses a 280 us reset period. A zero-length pulse
// is an end marker in esp-hal and must not be used as the reset pulse itself.
const RESET_LOW: PulseCode = PulseCode::new(Level::Low, 1400, Level::Low, 1400);

impl Ws2812 {
    pub fn new(
        rmt: esp_hal::peripherals::RMT<'static>,
        pin: esp_hal::peripherals::GPIO8<'static>,
    ) -> Self {
        let rmt = Rmt::new(rmt, Rate::from_mhz(80)).unwrap().into_async();

        let channel = rmt
            .channel0
            .configure_tx(
                &TxChannelConfig::default()
                    .with_memsize(1)
                    .with_clk_divider(7)
                    .with_idle_output(true)
                    .with_idle_output_level(Level::Low),
            )
            .unwrap()
            .with_pin(pin);

        Self { channel }
    }

    // This board's onboard LED is observed to accept RGB wire order.
    // Keep the public API and the wire order identical so its colors
    // match the LCD's logical RGB colors.
    pub async fn send_pixel(&mut self, color: Rgb888) {
        let mut codes = [BIT0; 26];
        let mut idx = 0;

        for byte in [color.r(), color.g(), color.b()] {
            for bit in (0..8).rev() {
                codes[idx] = if byte & (1u8 << bit) != 0 { BIT1 } else { BIT0 };
                idx += 1;
            }
        }

        codes[24] = RESET_LOW;
        codes[25] = PulseCode::end_marker();

        self.channel.transmit(&codes).await.unwrap();
    }
}
