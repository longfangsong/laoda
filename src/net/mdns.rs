//! mDNS 应答任务：广播 `laoda.local` 主机与 `_laoda-push._tcp` :5005 服务（设计文档 §8）。
//!
//! 工作站无需知道设备 IP：直接推 `laoda.local`（macOS / Linux+avahi 原生解析）。
//! A 记录取自 DHCP 租约（`wait_config_up` + `config_v4`，见 `stack.rs` 注释）。
//! hick 引擎无"更新记录"API（只有 register/unregister），DHCP 换租约时 mDNS
//! 会继续广播旧地址——家庭网络很少发生，重刷固件即修复。

use core::mem::MaybeUninit;

use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpListenEndpoint, Ipv4Address, Stack};
use embassy_time::{Duration, Timer};
use hick_embassy::MdnsState;
use log::{error, info};
use mdns_proto::{EndpointConfig, Name, ServiceRecords, ServiceSpec};
use rand_core::TryRng;
use static_cell::StaticCell;

use crate::net::push::PUSH_PORT;

const MDNS_PORT: u16 = 5353;
const GROUP: [u8; 4] = [224, 0, 0, 251];
const BUF: usize = 1500; // 1500 MTU 下 mDNS 数据报上限，与 hick 测试同尺寸

static RX_META: StaticCell<[PacketMetadata; 1]> = StaticCell::new();
static TX_META: StaticCell<[PacketMetadata; 1]> = StaticCell::new();
// MaybeUninit 包装，避免 1500 字节临时数组触发 clippy::large_stack_frames
static RX_BUF: StaticCell<MaybeUninit<[u8; BUF]>> = StaticCell::new();
static TX_BUF: StaticCell<MaybeUninit<[u8; BUF]>> = StaticCell::new();
static SCRATCH: StaticCell<MaybeUninit<[u8; BUF]>> = StaticCell::new();

/// 每个调用点只执行一次（任务启动时），此后缓冲独占；初始值无意义（收发前由协议覆盖）。
#[allow(
    clippy::mut_from_ref,
    reason = "StaticCell 内部用裸指针存放，init 后独占，同 embassy 官方示例"
)]
fn init_buf(slot: &'static StaticCell<MaybeUninit<[u8; BUF]>>) -> &'static mut [u8] {
    // SAFETY: init 仅调用一次；MaybeUninit 无需初始化
    unsafe { slot.init(MaybeUninit::uninit()).assume_init_mut() }
}

/// SplitMix64：mDNS 只用它做探测平票种子/查询事务号，固定种子即可。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

// rand_core 0.10 有 blanket impl：TryRng<Error=Infallible> 自动满足 Rng
impl TryRng for Rng {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.next() as u32)
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.next())
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        let mut i = 0;
        while i < dst.len() {
            let v = self.next();
            for j in 0..8u32 {
                if i + j as usize >= dst.len() {
                    break;
                }
                dst[i + j as usize] = (v >> (8 * j)) as u8;
            }
            i += 8;
        }
        Ok(())
    }
}

#[embassy_executor::task]
pub async fn mdns_task(stack: Stack<'static>) -> ! {
    stack.wait_config_up().await;
    let Some(cfg) = stack.config_v4() else {
        error!("mDNS: 无 IPv4 配置，无法广播");
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    };
    let ip = cfg.address.address();

    let mut socket = UdpSocket::new(
        stack,
        RX_META.init([PacketMetadata::EMPTY; 1]),
        init_buf(&RX_BUF),
        TX_META.init([PacketMetadata::EMPTY; 1]),
        init_buf(&TX_BUF),
    );
    if let Err(e) = socket.bind(IpListenEndpoint {
        addr: None,
        port: MDNS_PORT,
    }) {
        error!("mDNS 绑定 :{MDNS_PORT} 失败: {e:?}");
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    }
    if let Err(e) = stack.join_multicast_group(Ipv4Address::from(GROUP)) {
        error!("mDNS 加入组播组 224.0.0.251 失败: {e:?}");
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    }

    let state = MdnsState::new(EndpointConfig::new(), Rng(0x51a0_da0d));
    let mut records = ServiceRecords::new(
        Name::try_from_str("_laoda-push._tcp.local.").unwrap(),
        Name::try_from_str("laoda._laoda-push._tcp.local.").unwrap(),
        Name::try_from_str("laoda.local.").unwrap(),
        PUSH_PORT,
        120,
    );
    records.add_a(ip);
    // 堆用量见 crate::heap 的周期上报（用 esp-alloc 的 max_usage 看峰值，
    // 比在这里探一次瞬时余量准）
    match state.register_service(ServiceSpec::new(records)) {
        Ok(_) => info!("mDNS 广播 laoda.local → {ip}"),
        Err(e) => error!("mDNS 注册服务失败: {e:?}"),
    }
    state.run(Some(&mut socket), None, init_buf(&SCRATCH)).await
}
