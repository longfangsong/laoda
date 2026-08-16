#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_embedded_hal::shared_bus::asynch::spi::SpiDeviceWithConfig;
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
use laoda::data::Freshness;
use laoda::data::STATE;
use laoda::data::countdown::build_items;
use laoda::device::lcd::Lcd;
use laoda::device::tf;
use laoda::driver::ws2812::Ws2812;
use laoda::net;
use laoda::ui::page::claude_usage::{ClaudeUsage, UsageData};
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

/// 倒计时页的 `Day` 以秒计，最多每秒重绘一次（事件驱动边界见设计文档 §10/§15 第 8 步）
const REDRAW_TICK: Duration = Duration::from_secs(1);
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

    // 96KB，拆成两个区（esp-alloc 最多 3 个，跨区分配自动回退，等效单块 96KB）：
    // 64KB 放 RECLAIMED（bootloader 腾出的 dram2_seg，否则整段空转），
    // 余下 32KB 才占主 DRAM——主 DRAM 的 .bss 因此少 64KB，全转成栈余量。
    // 编译期实测：.bss 254856 → 189328，.stack 121928 → 187456。
    //
    // 尺寸别再往下砍。真机 70 分钟持续负载 soak（每 30s 一条用量推送，
    // 109/109 acked）实测：峰值 81900/98304，稳态 ~74.5KB（会回落，不是泄漏）。
    // esp-alloc 先填 region0，region0 满了才溢出到 region1，所以峰值时
    // region1 要吃 81900-65536 ≈ 16.4KB——把下面这 32768 砍到 16384 会当场
    // 分配失败。当前余量 16.4KB（16.7%），留给碎片和罕见事件（重连风暴、
    // DHCP 续租）刚好。堆用量见 crate::heap 的周期上报。
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);
    esp_alloc::heap_allocator!(size: 32768);

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
    spawner.spawn(net::push::push_task(net_stack).unwrap());
    spawner.spawn(net::mdns::mdns_task(net_stack).unwrap());

    // 堆用量周期上报（峰值），压缩上面两个 heap_allocator 尺寸的依据
    spawner.spawn(laoda::heap::heap_report_task().unwrap());

    // 串口调试接口（仅 debug 构建）：走 Type-C 原生 USB（USB-Serial-JTAG，
    // 与日志同一端口），`time <unix-秒>` 覆盖 NTP 时间，见 laoda::debug
    #[cfg(debug_assertions)]
    {
        let usj = esp_hal::usb_serial_jtag::UsbSerialJtag::new(peripherals.USB_DEVICE);
        spawner.spawn(laoda::debug::debug_console_task(usj).unwrap());
    }

    let ws2812 = Ws2812::new(peripherals.RMT, peripherals.GPIO8);
    spawner.spawn(rainbow_task(ws2812).unwrap());

    // SPI2（Waveshare ESP32-C6-LCD-1.47）：MOSI=GPIO6, SCLK=GPIO7 由 LCD 和
    // TF 卡槽共用；LCD 独占 CS=GPIO14, DC=GPIO15, RST=GPIO21, 背光=GPIO22，
    // TF 卡独占 MISO=GPIO5, CS=GPIO4。
    //
    // rx 缓冲 512 字节：TF 卡一次收发一整块（512B），而 `SpiDmaBus` 的
    // `transfer_in_place_async` 是按 tx 缓冲切块、把整块长度交给 rx 通道的，
    // rx 比一块小就会报 DMA 长度错。LCD 只写不读，不受影响。
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(512, 16384);
    let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
    let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();
    // 总线上的频率由每个设备在自己的事务里设（见 device::lcd::LcdSpi），
    // 这里给的只是初值。
    let lcd_spi_config = SpiConfig::default()
        .with_frequency(Rate::from_mhz(80))
        .with_mode(Mode::_0);
    let spi = Spi::new(peripherals.SPI2, lcd_spi_config)
        .unwrap()
        .with_sck(peripherals.GPIO7)
        .with_mosi(peripherals.GPIO6)
        .with_miso(peripherals.GPIO5)
        .with_dma(peripherals.DMA_CH0)
        .with_buffers(dma_rx_buf, dma_tx_buf)
        .into_async();
    let spi_bus = SPI_BUS.init(Mutex::new(spi));

    // 共用总线上不能有第二个设备被选中：先把 LCD 的片选拉到无效电平（高），
    // 再跟 TF 卡说话。这个 Output 会一直活到下面交给 LCD 的 SpiDeviceWithConfig。
    let lcd_cs = Output::new(peripherals.GPIO14, Level::High, OutputConfig::default());

    // TF 卡是可选外设：挂上了就记一行开机日志，没插卡/没格式化只打日志不影响其它功能。
    // 放在 LCD 之前：卡的初始化要在 400kHz 上握手，此时总线还没被刷屏任务占着。
    let tf_cs = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());
    let tf = tf::mount_optional(spi_bus, tf_cs).await;
    if let Some(tf) = &tf {
        tf.write_boot_record().await;
    }

    let lcd_spi = SpiDeviceWithConfig::new(spi_bus, lcd_cs, lcd_spi_config);
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
    // 两页都是实时数据：倒计时条目从 NTP 时钟现算（未对时显示 `--` 占位），
    // 用量从 STATE 读（push_task 写入，从未收到时显示 `--`）。
    let mut count_down = CountDown::new();

    let mut screen = Screen::CountDown;
    let mut next_tick = Instant::now() + REDRAW_TICK;
    let mut next_auto_switch = Instant::now() + AUTO_SWITCH;
    loop {
        // 两页共用同一次 STATE 快照
        let state = STATE.anon_receiver().try_get();
        let data = match state.and_then(|s| s.clock) {
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
        let usage = state
            .map(|s| UsageData {
                values: s.usage,
                freshness: s.freshness(),
            })
            .unwrap_or(UsageData {
                values: [0; 3],
                freshness: Freshness::Unknown,
            });

        let mut fb = lcd.frame().await;
        match screen {
            Screen::CountDown => count_down.draw(&mut *fb, &data).unwrap(),
            Screen::ClaudeUsage => ClaudeUsage::draw(&mut *fb, &usage).unwrap(),
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
            next_tick += REDRAW_TICK;
        }
        if now >= next_auto_switch {
            screen = screen.next();
            next_auto_switch = now + AUTO_SWITCH;
        }
    }
}
