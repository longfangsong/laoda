//! 应用全局状态：wifi / sntp / push 多个生产者写，渲染循环与 LED 只读。
//!
//! 倒计时条目不进状态——它是 `clock` 的纯函数，渲染时现算（设计文档 §6）。

mod clock;

pub mod countdown;

pub use clock::Clock;

/// 用量数据新鲜度（设计文档 §2：距上次推送 > 15 分钟为过期）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// 开机以来从未收到推送
    Unknown,
    Fresh,
    Stale,
}

/// WiFi 链路状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Online,
    Offline,
}

#[derive(Clone, Copy, Debug)]
pub struct AppState {
    /// 本地时钟；`None` = 尚未对时成功
    pub clock: Option<Clock>,
    /// Session / Week / Fable 用量百分比 0..=100（push 模块实现前恒为 0，勿直接显示）
    pub usage: [u8; 3],
    /// 最近一次用量推送的时刻；`None` = 从未收到
    pub usage_at: Option<embassy_time::Instant>,
    pub link: LinkState,
}

impl AppState {
    pub fn freshness(&self) -> Freshness {
        match self.usage_at {
            None => Freshness::Unknown,
            Some(at) if embassy_time::Instant::now() - at > STALE_AFTER => Freshness::Stale,
            Some(_) => Freshness::Fresh,
        }
    }
}

/// 推送新鲜度窗口
pub const STALE_AFTER: embassy_time::Duration = embassy_time::Duration::from_secs(15 * 60);

/// 全局状态。`Watch` 语义正好：多生产者各写最新值，消费者只关心当前值。
/// 用 [`modify_state`] 写，用 `STATE.anon_receiver().try_get()` 读。
pub static STATE: embassy_sync::watch::Watch<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    AppState,
    2,
> = embassy_sync::watch::Watch::new_with(AppState {
    clock: None,
    usage: [0; 3],
    usage_at: None,
    link: LinkState::Offline,
});

/// 更新全局状态的一部分。embassy-sync 0.8 的 `send_modify` 闭包拿到的是
/// `&mut Option<T>`，这里包掉这个噪声。
pub fn modify_state(f: impl Fn(&mut AppState)) {
    STATE.sender().send_modify(|s| {
        if let Some(s) = s {
            f(s);
        }
    });
}
