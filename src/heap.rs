//! 堆用量观测。
//!
//! 堆本身在 `main` 里用 `esp_alloc::heap_allocator!` 建（分 RECLAIMED 与主
//! DRAM 两个区），这里只负责把用量打出来，供压缩堆尺寸时做依据。
//!
//! 关键是 **峰值**（`max_usage`）而不是某一瞬间的占用：WiFi 在连接/重连时
//! 才会冲到高位，开机后立刻采样看不到。峰值统计由 esp-alloc 的
//! `internal-heap-stats` feature 提供（见 Cargo.toml），代价是每次
//! alloc/free 多几条整数指令——本机分配极少，可忽略。

use embassy_time::{Duration, Timer};
use log::info;

/// 首次上报前的等待：让 WiFi 关联 + DHCP + mDNS 注册这波分配先跑完。
const FIRST_REPORT_DELAY: Duration = Duration::from_secs(30);
/// 之后的上报间隔。峰值只增不减，稀疏采样即可。
const REPORT_INTERVAL: Duration = Duration::from_secs(300);

/// 周期性打印堆用量。按 `region` 分别打印，能看出 RECLAIMED 那 64KB 是否
/// 真的被用起来了（若它常年 used=0，说明主 DRAM 那 32KB 就够，可再压）。
#[embassy_executor::task]
pub async fn heap_report_task() -> ! {
    Timer::after(FIRST_REPORT_DELAY).await;
    loop {
        let stats = esp_alloc::HEAP.stats();
        info!(
            "heap: 峰值 {}/{} 字节（当前 {}）",
            stats.max_usage, stats.size, stats.current_usage
        );
        for (i, region) in stats.region_stats.iter().flatten().enumerate() {
            info!("heap: region{i} used {}/{} 字节", region.used, region.size);
        }
        Timer::after(REPORT_INTERVAL).await;
    }
}
