//! 本地时钟（设计文档 §4）：基准 Unix 秒 + 单调时钟外推，不用 RTC。
//!
//! 晶振 6 小时漂移远小于分钟级显示精度，所以同步后不需要高频校时。

use embassy_time::Instant;
use time::{OffsetDateTime, UtcOffset};

/// 固定本地时区，不自动切换夏令时（设计文档 §2）。
/// 偏移秒数来自 LAODA_TZ_OFFSET 编译期变量，默认 CEST（UTC+2，夏令时）。
/// 自动夏令时切换见 docs/backlog.md。
fn tz() -> UtcOffset {
    const DEFAULT_OFFSET_SECS: i32 = 2 * 3600;
    let secs = option_env!("LAODA_TZ_OFFSET")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(DEFAULT_OFFSET_SECS);
    UtcOffset::from_whole_seconds(secs).expect("LAODA_TZ_OFFSET 超出 ±24h")
}

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// 基准时刻的 Unix 秒
    epoch_at_ref: u64,
    /// 取得基准时的单调时刻
    instant_ref: Instant,
}

impl Clock {
    /// 以给定的 Unix 秒为基准建立时钟
    pub fn from_unix(unix_secs: u64) -> Self {
        Self {
            epoch_at_ref: unix_secs,
            instant_ref: Instant::now(),
        }
    }

    /// 当前 Unix 秒
    pub fn now_unix(&self) -> u64 {
        self.epoch_at_ref
            .saturating_add((Instant::now() - self.instant_ref).as_secs())
    }

    /// 当前本地时间
    pub fn now_local(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(self.now_unix() as i64)
            .expect("unix 秒在合理范围")
            .to_offset(tz())
    }
}
