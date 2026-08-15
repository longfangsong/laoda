#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice as SharedSpiDevice;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant, Timer, with_deadline};
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::dma::{DmaRxBuf, DmaTxBuf};
use esp_hal::dma_buffers;
use esp_hal::gpio::DriveMode;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::ledc::channel::ChannelIFace;
use esp_hal::ledc::timer::TimerIFace;
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed, channel, timer};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi, SpiDmaBus};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use laoda::data::STATE;
use laoda::data::countdown::build_items;
use laoda::device::lcd::Lcd;
use laoda::driver::ws2812::Ws2812;
use laoda::net;
use laoda::ui::page::claude_usage::{ClaudeUsage, GAUGE_COUNT, UsageItem};
use laoda::ui::page::count_down::{CountDown, CountDownData};
use laoda::ui::theme;
use log::info;
use static_cell::StaticCell;

static SPI_BUS: StaticCell<Mutex<NoopRawMutex, SpiDmaBus<'static, esp_hal::Async>>> =
    StaticCell::new();

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// 用量页面上的三个仪表盘。百分比是初值，demo 里每秒会被覆盖。
const USAGE: [UsageItem; GAUGE_COUNT] = [
    UsageItem::new("Session", 0.42),
    UsageItem::new("Week", 0.77),
    UsageItem::new("Fable", 0.13),
];

/// demo 数据推进的间隔
const DATA_TICK: Duration = Duration::from_secs(1);
/// 无人按键时自动切页的间隔
const AUTO_SWITCH: Duration = Duration::from_secs(5);
/// 按键去抖窗口
const DEBOUNCE: Duration = Duration::from_millis(30);

/// 当前显示哪一页
#[derive(Clone, Copy)]
enum Screen {
    CountDown,
    ClaudeUsage,
}

impl Screen {
    fn next(self) -> Self {
        match self {
            Self::CountDown => Self::ClaudeUsage,
            Self::ClaudeUsage => Self::CountDown,
        }
    }
}

/// 等一次有效按下：下降沿之后再确认引脚仍是低电平，滤掉抖动。
/// 按住不放只算一次（要再触发得先松手产生新的下降沿）。
async fn wait_for_press(button: &mut Input<'_>) {
    loop {
        button.wait_for_falling_edge().await;
        Timer::after(DEBOUNCE).await;
        if button.is_low() {
            return;
        }
    }
}

/// 色环上的位置（0-255）转 RGB，红→绿→蓝→红 连续渐变
fn wheel(pos: u8) -> Rgb888 {
    match pos {
        0..=84 => Rgb888::new(255 - pos * 3, pos * 3, 0),
        85..=169 => Rgb888::new(0, 255 - (pos - 85) * 3, (pos - 85) * 3),
        170..=255 => Rgb888::new((pos - 170) * 3, 0, 255 - (pos - 170) * 3),
    }
}

/// 板载 WS2812 走色环，50ms 一步（整圈 ~12.8s）
#[embassy_executor::task]
async fn rainbow_task(mut led: Ws2812) {
    let mut pos: u8 = 0;
    loop {
        led.send_pixel(wheel(pos)).await;
        pos = pos.wrapping_add(1);
        Timer::after(Duration::from_millis(50)).await;
    }
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.3.0
    // generator parameters: --chip esp32c6 -o esp32c6-mini-1 -o unstable-hal -o alloc -o wifi -o embassy -o esp-backtrace -o log -o ci -o zed -o vscode

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO4
    // - GPIO5
    // - GPIO8
    // - GPIO9
    // - GPIO15
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _ = peripherals.GPIO24;
    let _ = peripherals.GPIO25;
    let _ = peripherals.GPIO26;
    let _ = peripherals.GPIO27;
    let _ = peripherals.GPIO28;
    let _ = peripherals.GPIO29;
    let _ = peripherals.GPIO30;

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    info!("Embassy initialized!");

    // WiFi + 协议栈 + NTP 对时：链路状态与本地时钟写入 STATE
    let (wifi_controller, interfaces) = esp_radio::wifi::new(peripherals.WIFI, Default::default())
        .expect("Failed to initialize Wi-Fi controller");
    let (net_stack, net_runner) = net::stack::new(interfaces.station);
    spawner.spawn(net::stack::net_task(net_runner).unwrap());
    spawner.spawn(net::wifi::wifi_task(wifi_controller).unwrap());
    spawner.spawn(net::sntp::sntp_task(net_stack).unwrap());

    // 串口调试接口（仅 debug 构建）：走 Type-C 原生 USB（USB-Serial-JTAG，
    // 与日志同一端口），`time <unix-秒>` 覆盖 NTP 时间，见 laoda::debug
    #[cfg(debug_assertions)]
    {
        let usj = esp_hal::usb_serial_jtag::UsbSerialJtag::new(peripherals.USB_DEVICE);
        spawner.spawn(laoda::debug::debug_console_task(usj).unwrap());
    }

    let ws2812 = Ws2812::new(peripherals.RMT, peripherals.GPIO8);
    spawner.spawn(rainbow_task(ws2812).unwrap());

    // LCD（Waveshare ESP32-C6-LCD-1.47）：MOSI=GPIO6, SCLK=GPIO7,
    // CS=GPIO14, DC=GPIO15, RST=GPIO21, 背光=GPIO22。
    // SPI 总线用共享 mutex，板上 TF 卡槽与 LCD 共用这条总线。
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(64, 16384);
    let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
    let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(80))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO7)
    .with_mosi(peripherals.GPIO6)
    .with_dma(peripherals.DMA_CH0)
    .with_buffers(dma_rx_buf, dma_tx_buf)
    .into_async();
    let spi_bus = SPI_BUS.init(Mutex::new(spi));
    let lcd_cs = Output::new(peripherals.GPIO14, Level::High, OutputConfig::default());
    let lcd_spi = SharedSpiDevice::new(spi_bus, lcd_cs);
    let lcd_dc = Output::new(peripherals.GPIO15, Level::Low, OutputConfig::default());
    let lcd_rst = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());

    let mut ledc = Ledc::new(peripherals.LEDC);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut backlight_timer = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    backlight_timer
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(24),
        })
        .unwrap();
    let mut backlight = ledc.channel(channel::Number::Channel0, peripherals.GPIO22);
    backlight
        .configure(channel::config::Config {
            timer: &backlight_timer,
            duty_pct: 100,
            drive_mode: DriveMode::PushPull,
        })
        .unwrap();

    info!("Peripherals set up, initializing LCD...");
    let mut lcd = Lcd::new(&spawner, lcd_spi, lcd_dc, lcd_rst, backlight).await;
    info!("LCD initialized");

    lcd.frame().await.clear(theme::BACKGROUND).unwrap();

    // BOOT 键（GPIO9，按下拉低，板上已有上拉，这里再开内部上拉保险）
    let mut button = Input::new(
        peripherals.GPIO9,
        InputConfig::default().with_pull(Pull::Up),
    );

    // 两页每 5s 自动轮换，按一下 BOOT 立刻切页。
    // 倒计时页为实时数据：每帧从 NTP 时钟构建条目（未对时显示 `--` 占位）；
    // 用量页仍为 demo 数据（push 模块未实现，设计文档 §15 第 6 步）。
    let mut count_down = CountDown::new();
    let mut claude_usage = ClaudeUsage::new(USAGE);

    let mut screen = Screen::CountDown;
    let mut tick: u16 = 0;
    let mut next_tick = Instant::now() + DATA_TICK;
    let mut next_auto_switch = Instant::now() + AUTO_SWITCH;
    loop {
        let data = match STATE.anon_receiver().try_get().and_then(|s| s.clock) {
            Some(clock) => {
                let dt = clock.now_local();
                CountDownData::Ready {
                    month: u8::from(dt.date().month()),
                    day: dt.date().day(),
                    items: build_items(dt),
                }
            }
            None => CountDownData::Unknown,
        };

        for i in 0..GAUGE_COUNT {
            let period = 60 / (i as u16 + 1);
            claude_usage.set_percentage(i, (tick % period) as f32 / period as f32);
        }

        let mut fb = lcd.frame().await;
        match screen {
            Screen::CountDown => count_down.draw(&mut *fb, &data).unwrap(),
            Screen::ClaudeUsage => claude_usage.draw(&mut *fb).unwrap(),
        }
        drop(fb);

        // 睡到下一次数据推进或自动切页，期间按键可以随时把我们叫醒。
        // next_tick 是绝对时刻，所以按键打断不会让数据的节奏跑偏。
        let deadline = next_tick.min(next_auto_switch);
        if with_deadline(deadline, wait_for_press(&mut button))
            .await
            .is_ok()
        {
            screen = screen.next();
            next_auto_switch = Instant::now() + AUTO_SWITCH;
            info!("button pressed, switching page");
            continue;
        }

        let now = Instant::now();
        if now >= next_tick {
            tick = tick.wrapping_add(1);
            next_tick += DATA_TICK;
        }
        if now >= next_auto_switch {
            screen = screen.next();
            next_auto_switch = now + AUTO_SWITCH;
        }
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}
