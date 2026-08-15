//! SNTP 客户端与对时任务（设计文档 §9 / §10 `sntp_task`）。
//!
//! 不引第三方 NTP crate：客户端只需发 48 字节请求、读回包里的 transmit
//! timestamp。取 t3 不做 RTT 偏移修正——局域网往返毫秒级，远小于分钟级
//! 显示精度。
//!
//! 策略（设计文档 §4）：开机立即同步；失败按 1s→2s→4s…→60s 封顶退避；
//! 成功后每 6 小时重新同步一次。

use embassy_net::dns::DnsQueryType;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address, Stack};
use embassy_time::{Duration, Timer, with_timeout};
use log::{debug, error, info, warn};
use static_cell::StaticCell;

use crate::data::{Clock, LinkState, STATE, modify_state};

/// NTP 纪元（1900-01-01）到 Unix 纪元（1970-01-01）的秒数
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;
const NTP_PORT: u16 = 123;
const NTP_HOST: &str = "pool.ntp.org";

/// DNS 失败时的回落服务器（从瑞典实测可达）。
/// pool.ntp.org 在瑞典会解析到本地池（Telia），fallback 为欧洲/全球节点。
const FALLBACK_SERVERS: [IpAddress; 3] = [
    IpAddress::Ipv4(Ipv4Address::new(162, 159, 200, 1)), // time.cloudflare.com
    IpAddress::Ipv4(Ipv4Address::new(91, 189, 91, 157)), // ntp.ubuntu.com（法兰克福）
    IpAddress::Ipv4(Ipv4Address::new(216, 239, 35, 12)), // time.google.com（全球）
];

const DNS_TIMEOUT: Duration = Duration::from_secs(10);
const NTP_TIMEOUT: Duration = Duration::from_secs(5);
const RESYNC_EVERY: Duration = Duration::from_secs(6 * 3600);
/// 失败退避上限（秒）
const MAX_BACKOFF_SECS: u64 = 60;

/// UDP 缓冲区：NTP 载荷固定 48 字节，收发各 1 个包足够
static RX_META: StaticCell<[PacketMetadata; 1]> = StaticCell::new();
static RX_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static TX_META: StaticCell<[PacketMetadata; 1]> = StaticCell::new();
static TX_BUF: StaticCell<[u8; 48]> = StaticCell::new();

/// 对时任务：成功后把 [`Clock`] 写入 [`STATE`]，永不退出。
#[embassy_executor::task]
pub async fn sntp_task(stack: Stack<'static>) -> ! {
    let mut socket = UdpSocket::new(
        stack,
        RX_META.init([PacketMetadata::EMPTY; 1]),
        RX_BUF.init([0u8; 64]),
        TX_META.init([PacketMetadata::EMPTY; 1]),
        TX_BUF.init([0u8; 48]),
    );
    // 必须绑源端口 123：Cloudflare/Google 等大型 NTP 服务对非常规源端口
    // 回 stratum 0 拒服包（kiss-o'-death），池内服务器也普遍过滤临时端口
    if let Err(e) = socket.bind(IpListenEndpoint {
        addr: None,
        port: NTP_PORT,
    }) {
        error!("ntp socket bind 失败: {e:?}");
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    }

    let mut backoff: u64 = 1;
    loop {
        // 链路没起来时 send_to 假成功、包直接丢，只会白等 5s 超时。
        // 关联成功后 DHCP 可能还在跑，首包仍可能失败，靠 1s 退避重试抹平
        if STATE.anon_receiver().try_get().map(|s| s.link) != Some(LinkState::Online) {
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }
        match sync_once(&stack, &mut socket).await {
            Some(unix) => {
                // 串口调试接口设置的时间覆盖优先（仅 debug 构建存在该模块）
                #[cfg(debug_assertions)]
                if crate::debug::time_override_active() {
                    debug!("ntp 对时成功但被忽略：串口时间覆盖生效 (unix={unix})");
                    backoff = 1;
                    Timer::after(RESYNC_EVERY).await;
                    continue;
                }
                modify_state(|s| s.clock = Some(Clock::from_unix(unix)));
                info!("ntp 对时成功: unix={unix}");
                backoff = 1;
                Timer::after(RESYNC_EVERY).await;
            }
            None => {
                debug!("ntp 对时失败，{backoff}s 后重试");
                Timer::after(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
            }
        }
    }
}

/// 一轮对时：先 DNS 解析 NTP_HOST，DNS 不可用再逐个试硬编码服务器。
/// 返回 Unix 秒。
async fn sync_once(stack: &Stack<'static>, socket: &mut UdpSocket<'static>) -> Option<u64> {
    if let Some(ip) = resolve(stack).await {
        let server = IpEndpoint::new(ip.into(), NTP_PORT);
        if let Ok(unix) = ntp_query(socket, server).await {
            return Some(unix);
        }
    }
    for ip in FALLBACK_SERVERS {
        if let Ok(unix) = ntp_query(socket, IpEndpoint::new(ip, NTP_PORT)).await {
            return Some(unix);
        }
    }
    None
}

/// 解析 NTP 服务器地址。DNS 未配置（DHCP 未完成）或超时时返回 None。
async fn resolve(stack: &Stack<'static>) -> Option<Ipv4Address> {
    let addrs = match with_timeout(DNS_TIMEOUT, stack.dns_query(NTP_HOST, DnsQueryType::A)).await {
        Ok(Ok(addrs)) => addrs,
        _ => return None,
    };
    match addrs.first().copied() {
        Some(IpAddress::Ipv4(v4)) => Some(v4),
        _ => None,
    }
}

/// 发一次 NTP client 请求，校验并解析响应里的 transmit timestamp（t3）。
async fn ntp_query(socket: &UdpSocket<'static>, server: IpEndpoint) -> Result<u64, ()> {
    let mut req = [0u8; 48];
    req[0] = 0x1B; // LI=0, VN=3, Mode=3 (client)
    if let Err(e) = socket.send_to(&req, server).await {
        warn!("ntp 发送失败 {server}: {e:?}");
        return Err(());
    }

    let mut buf = [0u8; 48];
    let (n, _) = match with_timeout(NTP_TIMEOUT, socket.recv_from(&mut buf)).await {
        Err(_) => {
            warn!("ntp 超时 {server}（已发出，5s 无响应）");
            return Err(());
        }
        Ok(Err(e)) => {
            warn!("ntp 接收错误 {server}: {e:?}");
            return Err(());
        }
        Ok(Ok(v)) => v,
    };
    // 校验：server 模式、stratum 1..=15（byte1 整字节）、t3 非零
    if n < 48 || (buf[0] & 0x07) != 4 || !(1..=15).contains(&buf[1]) || buf[40..44] == [0u8; 4] {
        warn!(
            "ntp 响应非法 {server}: n={n} b0={:02x} stratum={}",
            buf[0], buf[1]
        );
        return Err(());
    }

    let secs = u32::from_be_bytes(buf[40..44].try_into().unwrap()) as u64;
    secs.checked_sub(NTP_UNIX_OFFSET).ok_or(())
}
