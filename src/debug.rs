//! 串口调试接口——仅 `debug_assertions` 开启时编译（`cargo build` 有效，
//! `--release` 整模块移除）。
//!
//! 走板载 Type-C 的原生 USB（USB-Serial-JTAG，即 `/dev/cu.usbmodemXXXX`）。
//! 用法与 esp32c6-lab 原型一致：Blocking + 轮询，不挂 esp-hal 中断
//! （esp-rtos 环境下 async 绑定不可靠）；回显走驱动自己的 TX。
//!
//! 协议（回车或换行结束，支持退格）：
//! - `time <unix-秒>`——覆盖本地时钟；覆盖期间 NTP 对时结果被丢弃，
//!   直到 `time reset` 或重启
//! - `time`——打印当前 unix 秒
//! - `time reset`——清除覆盖，回到 NTP 对时（下次对时前页面显示 `--`）
//!
//! 示例：`echo "time $(date +%s)" > /dev/cu.usbmodemXXXX`

use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, Ordering};

use esp_hal::usb_serial_jtag::UsbSerialJtag;
use esp_hal::Blocking;

use crate::data::{Clock, STATE, modify_state};

/// 串口设置过时间覆盖后为 true，NTP 对时结果丢弃（sntp_task 检查）
static TIME_OVERRIDE: AtomicBool = AtomicBool::new(false);

/// sntp_task 用的检查
pub fn time_override_active() -> bool {
    TIME_OVERRIDE.load(Ordering::Relaxed)
}

#[embassy_executor::task]
pub async fn debug_console_task(mut usj: UsbSerialJtag<'static, Blocking>) -> ! {
    let mut line = [0u8; 48];
    let mut len = 0;
    loop {
        let byte = match usj.read_byte() {
            Ok(b) => b,
            // 低流量调试口：没数据就睡 10ms，不用中断
            Err(_) => {
                embassy_time::Timer::after(embassy_time::Duration::from_millis(10)).await;
                continue;
            }
        };
        match byte {
            b'\r' | b'\n' => {
                if len > 0 {
                    handle_line(&line[..len], &mut usj);
                }
                len = 0;
            }
            0x08 | 0x7f => len = len.saturating_sub(1),
            c if len < line.len() && (0x20..=0x7e).contains(&c) => {
                line[len] = c;
                len += 1;
            }
            _ => {}
        }
        if len == line.len() {
            handle_line(&line, &mut usj);
            len = 0;
        }
    }
}

fn handle_line(line: &[u8], usj: &mut UsbSerialJtag<'_, Blocking>) {
    let Ok(s) = core::str::from_utf8(line) else {
        return;
    };
    let s = s.trim();
    match s {
        "time" => match STATE.anon_receiver().try_get().and_then(|s| s.clock) {
            Some(clock) => {
                let _ = write!(usj, "debug: now unix={}\n", clock.now_unix());
            }
            None => {
                let _ = write!(usj, "debug: 时钟未设置（NTP 未对时且未串口覆盖）\n");
            }
        },
        _ if s.starts_with("time ") => match s["time ".len()..].trim() {
            "reset" => {
                TIME_OVERRIDE.store(false, Ordering::Relaxed);
                modify_state(|s| s.clock = None);
                let _ = write!(usj, "debug: 已清除时间覆盖，NTP 将重新对时\n");
            }
            "" => {
                let _ = write!(usj, "debug: 用法: time <unix-seconds> | time | time reset\n");
            }
            rest => match rest.parse::<u64>() {
                Ok(unix) => {
                    modify_state(|s| s.clock = Some(Clock::from_unix(unix)));
                    TIME_OVERRIDE.store(true, Ordering::Relaxed);
                    let _ = write!(
                        usj,
                        "debug: 时间已覆盖 unix={unix}，NTP 结果在 reset 前被忽略\n"
                    );
                }
                Err(_) => {
                    let _ = write!(usj, "debug: 不是 unix 秒数: {rest:?}\n");
                }
            },
        },
        other => {
            let _ = write!(
                usj,
                "debug: 未知命令 {other:?}（time <unix-seconds> | time | time reset）\n"
            );
        }
    }
}
