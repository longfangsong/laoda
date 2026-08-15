#![no_std]

/// Abstract drivers to external devices
/// Chip/bus/wire level, not whole device level
pub mod driver;

/// Device is a layer above driver
/// Device = driver + config for certain device on the dev board
pub mod device;

/// 应用全局状态：时钟、链路、用量
pub mod data;

/// 串口调试接口（仅 debug 构建编译）：串口发送时间覆盖 NTP
#[cfg(debug_assertions)]
pub mod debug;

/// 网络层：WiFi、协议栈、SNTP
pub mod net;

pub mod ui;

pub mod util;
