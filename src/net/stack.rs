//! embassy-net 协议栈初始化与 runner 任务（设计文档 §10 `net_task`）。

use esp_radio::wifi::Interface;
use static_cell::StaticCell;

/// socket 槽位：DHCP 1 + DNS 1 + SNTP 1。push 模块落地时再加 1。
const SOCK: usize = 3;

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
    let config = embassy_net::Config::dhcpv4(Default::default());
    let resources = RESOURCES.init(embassy_net::StackResources::new());
    embassy_net::new(driver, config, resources, RANDOM_SEED)
}

/// 协议栈 runner，永不退出。断线重连后 DHCP 由 embassy-net 自动重新发起。
#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, Interface<'static>>) -> ! {
    runner.run().await
}
