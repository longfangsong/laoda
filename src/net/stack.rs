//! embassy-net 协议栈初始化与 runner 任务（设计文档 §10 `net_task`）。
//!
//! IPv4 走 DHCP。租到的地址可从 `Stack::config_v4()` 读到（0.9.1 把 DHCP
//! 租约也存进 `static_v4`），mDNS 任务（§8）靠它填 A 记录，所以工作站
//! 无需知道设备 IP，直接推 `laoda.local`。

use esp_radio::wifi::Interface;
use static_cell::StaticCell;

/// socket 槽位：DHCP 1 + DNS 1 + SNTP 1 + push 1 + mDNS 1。
const SOCK: usize = 5;

static RESOURCES: StaticCell<embassy_net::StackResources<SOCK>> = StaticCell::new();

// 固定种子即可：smoltcp 只用它随机化初始端口，单 UDP 客户端无碰撞问题。
const RANDOM_SEED: u64 = 0x51a0_da0d;

/// 创建协议栈。`driver` 是 WiFi station 接口，runner 必须交给 [`net_task`] 常驻运行。
pub fn new(
    driver: Interface<'static>,
) -> (
    embassy_net::Stack<'static>,
    embassy_net::Runner<'static, Interface<'static>>,
) {
    let resources = RESOURCES.init(embassy_net::StackResources::new());
    embassy_net::new(driver, embassy_net::Config::dhcpv4(Default::default()), resources, RANDOM_SEED)
}

/// 协议栈 runner，永不退出。DHCP 在 runner 内处理，断线重连无需额外处理。
#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, Interface<'static>>) -> ! {
    runner.run().await
}
