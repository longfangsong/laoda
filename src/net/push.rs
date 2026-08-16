//! 用量推送接收任务（设计文档 §8 / §10 `push_task`）。
//!
//! 被动监听 UDP :5005，收工作机广播的定长文本行
//! `laoda1 <token> <session> <week> <fable> <epoch>\n`。
//! 有效包写 usage 进 STATE；NTP 尚未对时（`clock == None`）时同时采用包内
//! epoch 作为冗余时间源（设计文档 §4），NTP 一旦成功就以 NTP 为准。
//! 收到有效包向来源地址单播 `ok\n` 回执：工作机据此确认推送成功，并从回执
//! 学到设备 IP，之后可改单播（设计文档 §8）。

use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpListenEndpoint, Stack};
use embassy_time::{Duration, Instant, Timer};
use log::{debug, error, info, warn};
use static_cell::StaticCell;

use crate::data::{Clock, modify_state};
use crate::util::parse_usage_push;

/// 推送接收端口（设计文档 §8）
pub const PUSH_PORT: u16 = 5005;

/// 推送行实际只有几十字节；超过即整包丢弃
const MAX_LINE: usize = 96;

static RX_META: StaticCell<[PacketMetadata; 1]> = StaticCell::new();
static RX_BUF: StaticCell<[u8; MAX_LINE]> = StaticCell::new();
static TX_META: StaticCell<[PacketMetadata; 1]> = StaticCell::new();
static TX_BUF: StaticCell<[u8; 4]> = StaticCell::new(); // "ok\n"

/// 推送接收任务：校验 token、写 usage、回 ack。永不退出。
#[embassy_executor::task]
pub async fn push_task(stack: Stack<'static>) -> ! {
    let token = option_env!("LAODA_PUSH_TOKEN").unwrap_or_default();
    if token.is_empty() {
        error!("LAODA_PUSH_TOKEN 未配置：用量推送将被全部丢弃（复制 .env.example 为 .env 并填入）");
    }

    let mut socket = UdpSocket::new(
        stack,
        RX_META.init([PacketMetadata::EMPTY; 1]),
        RX_BUF.init([0u8; MAX_LINE]),
        TX_META.init([PacketMetadata::EMPTY; 1]),
        TX_BUF.init([0u8; 4]),
    );
    if let Err(e) = socket.bind(IpListenEndpoint {
        addr: None,
        port: PUSH_PORT,
    }) {
        error!("push socket bind 失败: {e:?}");
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    }

    loop {
        let mut buf = [0u8; MAX_LINE];
        let (n, from) = match socket.recv_from(&mut buf).await {
            Err(e) => {
                warn!("push 接收失败: {e:?}");
                continue;
            }
            Ok(v) => v,
        };
        let Ok(line) = core::str::from_utf8(&buf[..n]) else {
            debug!("push 包非 UTF-8，丢弃");
            continue;
        };
        let Some((usage, epoch)) = parse_usage_push(line, token) else {
            debug!("push 行非法，丢弃: {line:?}");
            continue;
        };
        modify_state(|s| {
            s.usage = usage;
            s.usage_at = Some(Instant::now());
            if s.clock.is_none() {
                s.clock = Some(Clock::from_unix(epoch));
            }
        });
        info!(
            "用量推送已接收: session={} week={} fable={}",
            usage[0], usage[1], usage[2]
        );
        match socket.send_to(b"ok\n", from).await {
            Ok(()) => info!("ack 已发送 → {from:?}"),
            Err(e) => warn!("ack 发送失败 → {from:?}: {e:?}"),
        }
    }
}
