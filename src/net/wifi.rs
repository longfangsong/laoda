//! WiFi 连接与断线重连（设计文档 §10 `wifi_task`）。
//!
//! 凭据编译期写入（设计文档 §8 非目标：无配网界面）。连接失败按
//! 1s→2s→4s…→60s 封顶退避；连上后挂起等待断线事件，再回到重连循环。

use embassy_time::{Duration, Timer};
use esp_radio::wifi::{Config, PowerSaveMode, WifiController, sta::StationConfig};
use log::{error, info, warn};

use crate::data::{LinkState, modify_state};

/// 退避上限（秒），与设计文档 §4 的 NTP 重试策略一致
const MAX_BACKOFF_SECS: u64 = 60;

fn set_link(link: LinkState) {
    modify_state(|s| s.link = link);
}

/// WiFi 连接任务，永不退出。
#[embassy_executor::task]
pub async fn wifi_task(mut controller: WifiController<'static>) -> ! {
    let ssid = option_env!("LAODA_WIFI_SSID").unwrap_or_default();
    let psk = option_env!("LAODA_WIFI_PSK").unwrap_or_default();
    if ssid.is_empty() || psk.is_empty() {
        error!("WiFi 凭据未配置：复制 .env.example 为 .env 并填入 SSID/密码");
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    }

    let station = Config::Station(
        StationConfig::default()
            .with_ssid(ssid)
            .with_password(psk.into()),
    );
    if let Err(e) = controller.set_config(&station) {
        error!("wifi 配置失败: {e:?}");
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    }
    // DTIM modem sleep，与 UDP 收发兼容（设计文档 §11）
    if let Err(e) = controller.set_power_saving(PowerSaveMode::Maximum) {
        warn!("wifi 省电模式设置失败: {e:?}");
    }

    let mut backoff: u64 = 1;
    loop {
        set_link(LinkState::Connecting);
        match controller.connect_async().await {
            Ok(info) => {
                info!("wifi 已连接: ssid={:?} channel={}", info.ssid, info.channel);
                set_link(LinkState::Online);
                backoff = 1;
                // 阻塞直到断线，然后回循环重连
                match controller.wait_for_disconnect_async().await {
                    Ok(d) => warn!("wifi 断开: {:?} reason={:?}", d.ssid, d.reason),
                    Err(e) => warn!("wifi 等待断线事件失败: {e:?}"),
                }
                set_link(LinkState::Offline);
            }
            Err(e) => {
                warn!("wifi 连接失败: {e:?}，{backoff}s 后重试");
                set_link(LinkState::Offline);
                Timer::after(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
            }
        }
    }
}
