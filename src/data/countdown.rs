//! 倒计时计算（设计文档 §5）。
//!
//! 全部纯函数：输入本地时区的 `OffsetDateTime`，输出条目。只依赖 `time`，
//! 可在 host 上单测（`host-tests/`，见设计文档 §15 第 3 步）。

use time::{Date, Duration, Month, OffsetDateTime, Time, UtcOffset, Weekday};

/// 一天秒数
pub const DAY_SECS: u32 = 86_400;
/// 工作时段 08:00–18:00 本地时间（设计文档 §2），按 00:00 起算的秒数
pub const WORK_START_SECS: u32 = 8 * 3600;
pub const WORK_END_SECS: u32 = 18 * 3600;
/// 一个工作日 = 10h = 600min
pub const WORK_DAY_SECS: u32 = WORK_END_SECS - WORK_START_SECS;
/// 一个工作周（周一 08:00 – 周五 18:00）= 50h
pub const WORK_WEEK_SECS: u32 = 5 * WORK_DAY_SECS;

/// SW 发布日与进度条起点（硬编码，设计文档 §3.1）
pub const SW_RELEASE_DATE: (i32, Month, u8) = (2027, Month::April, 1);
pub const SW_RELEASE_ORIGIN: (i32, Month, u8) = (2026, Month::January, 1);

/// 条目数与固定顺序：Year → Week → Day → SW Release（设计文档 §3.4）
pub const ITEM_COUNT: usize = 4;
pub const ITEM_LABELS: [&str; ITEM_COUNT] = ["Year", "Week", "Day", "SW Release"];

/// 显示单位。值一律向上取整（设计文档 §3.3）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Days,
    Hours,
    Minutes,
    Seconds,
}

impl Unit {
    const fn secs(self) -> u32 {
        match self {
            Self::Days => DAY_SECS,
            Self::Hours => 3600,
            Self::Minutes => 60,
            Self::Seconds => 1,
        }
    }

    /// 数值后面的单位字符，如 `2610H` / `3000M` / `36000S`
    pub const fn suffix(self) -> u8 {
        match self {
            Self::Days => b'D',
            Self::Hours => b'H',
            Self::Minutes => b'M',
            Self::Seconds => b'S',
        }
    }
}

/// 一个倒计时条目。进度条画「已经过去的比例」，文字右侧显示剩余量。
#[derive(Clone, Copy, Debug)]
pub struct CountDownItem {
    /// 标签，全 ASCII 不含空格（字体限制）
    pub label: &'static str,
    /// 剩余秒数
    pub remaining_secs: u32,
    /// 总量秒数，用来算已过去比例；为 0 时进度条视为空
    pub total_secs: u32,
    pub unit: Unit,
    /// Year 与 SW Release 始终可见（设计文档 §3.2）
    pub always_visible: bool,
}

impl CountDownItem {
    /// 按 unit 向上取整的显示值：剩余 1 秒显示 `1M`
    pub fn value(&self) -> u32 {
        self.remaining_secs.div_ceil(self.unit.secs())
    }

    /// 已经过去的比例，0.0..=1.0（钳位）
    pub fn elapsed(&self) -> f32 {
        if self.total_secs == 0 {
            return 0.0;
        }
        let left = self.remaining_secs.min(self.total_secs) as f32 / self.total_secs as f32;
        (1.0 - left).clamp(0.0, 1.0)
    }

    /// 剩余量 == 0 隐藏；always_visible 优先（设计文档 §3.2 统一规则）
    pub fn visible(&self) -> bool {
        self.always_visible || self.remaining_secs > 0
    }
}

fn is_workday(wd: Weekday) -> bool {
    matches!(
        wd,
        Weekday::Monday
            | Weekday::Tuesday
            | Weekday::Wednesday
            | Weekday::Thursday
            | Weekday::Friday
    )
}

fn secs_of_day(t: Time) -> u32 {
    t.hour() as u32 * 3600 + t.minute() as u32 * 60 + t.second() as u32
}

/// 指定日期的指定 00:00 起秒数时刻，offset 同 `now`
fn local_dt(date: Date, secs_of_day: u32, offset: UtcOffset) -> OffsetDateTime {
    let t = Time::from_hms(
        (secs_of_day / 3600) as u8,
        ((secs_of_day % 3600) / 60) as u8,
        (secs_of_day % 60) as u8,
    )
    .expect("工作时段为合法时刻");
    // with_time 在 time 0.3 是 infallible（日期已存在）
    date.with_time(t).assume_offset(offset)
}

/// Day（设计文档 §3.2）：工作日 08:00–18:00 之间为距 18:00 的秒数，否则 0
pub fn day_remaining_secs(now: OffsetDateTime) -> u32 {
    let s = secs_of_day(now.time());
    if !is_workday(now.weekday()) || !(WORK_START_SECS..WORK_END_SECS).contains(&s) {
        return 0;
    }
    WORK_END_SECS - s
}

/// 今日剩余工作秒数：非工作日 0；08:00 之前算完整 10h（设计文档 §5 刻意区分）
fn today_work_secs(now: OffsetDateTime) -> u32 {
    if !is_workday(now.weekday()) {
        return 0;
    }
    let s = secs_of_day(now.time());
    if s >= WORK_END_SECS {
        0
    } else if s < WORK_START_SECS {
        WORK_DAY_SECS
    } else {
        WORK_END_SECS - s
    }
}

/// [start, end) 内所有工作日落在工作时段内的秒数之和（按整天计）
fn work_secs_in_range(start: Date, end: Date) -> u32 {
    let mut total = 0;
    let mut d = start;
    while d < end {
        if is_workday(d.weekday()) {
            total += WORK_DAY_SECS;
        }
        d += Duration::days(1);
    }
    total
}

/// Week（设计文档 §3.2）：现在到本周五 18:00 落在工作时段内的秒数之和
pub fn week_remaining_secs(now: OffsetDateTime) -> u32 {
    let wd = now.weekday().number_from_monday(); // 周一=1 … 周日=7
    if wd > 5 {
        return 0;
    }
    today_work_secs(now) + (5 - wd) as u32 * WORK_DAY_SECS
}

/// Year（设计文档 §3.2）：现在到年底落在工作时段（周一~周五 08:00–18:00）内的秒数之和
pub fn year_remaining_secs(now: OffsetDateTime) -> u32 {
    let end = Date::from_calendar_date(now.year() + 1, Month::January, 1).expect("1 月 1 日存在");
    today_work_secs(now) + work_secs_in_range(now.date() + Duration::days(1), end)
}

/// Year 总量：今年全年落在工作时段内的秒数（进度条比例用）
pub fn year_total_secs(now: OffsetDateTime) -> u32 {
    let start = Date::from_calendar_date(now.year(), Month::January, 1).expect("1 月 1 日存在");
    let end = Date::from_calendar_date(now.year() + 1, Month::January, 1).expect("1 月 1 日存在");
    work_secs_in_range(start, end)
}

/// 发布日（自然日）
fn release_day() -> Date {
    let (y, m, d) = SW_RELEASE_DATE;
    Date::from_calendar_date(y, m, d).expect("发布日合法")
}

/// 发布时刻 = 发布日 18:00（同 `now` 的 offset）
pub fn release_target(now: OffsetDateTime) -> OffsetDateTime {
    local_dt(release_day(), WORK_END_SECS, now.offset())
}

/// SW Release（设计文档 §3.2）：现在到发布时刻落在工作时段（周一~周五 08:00–18:00）内的秒数，已过钳到 0
pub fn release_remaining_secs(now: OffsetDateTime) -> u32 {
    if now >= release_target(now) {
        return 0;
    }
    let end = release_day() + Duration::days(1); // 不含
    today_work_secs(now) + work_secs_in_range(now.date() + Duration::days(1), end)
}

/// 发布条总量：起点 00:00 → 发布时刻的全体工作时段秒数（设计文档 §3.1）
pub fn release_total_secs() -> u32 {
    let (y, m, d) = SW_RELEASE_ORIGIN;
    let origin = Date::from_calendar_date(y, m, d).expect("起点日合法");
    work_secs_in_range(origin, release_day() + Duration::days(1))
}

/// 组装四条条目（顺序固定，是否显示由渲染层按 [`CountDownItem::visible`] 决定）
pub fn build_items(now: OffsetDateTime) -> [CountDownItem; ITEM_COUNT] {
    [
        CountDownItem {
            label: ITEM_LABELS[0],
            remaining_secs: year_remaining_secs(now),
            total_secs: year_total_secs(now),
            unit: Unit::Hours, // 显示为本年度剩余工作小时数
            always_visible: true,
        },
        CountDownItem {
            label: ITEM_LABELS[1],
            remaining_secs: week_remaining_secs(now),
            total_secs: WORK_WEEK_SECS,
            unit: Unit::Minutes,
            always_visible: false,
        },
        CountDownItem {
            label: ITEM_LABELS[2],
            remaining_secs: day_remaining_secs(now),
            total_secs: WORK_DAY_SECS,
            unit: Unit::Seconds,
            always_visible: false,
        },
        CountDownItem {
            label: ITEM_LABELS[3],
            remaining_secs: release_remaining_secs(now),
            total_secs: release_total_secs(),
            unit: Unit::Hours, // 显示为距发布日的剩余工作小时数
            always_visible: true,
        },
    ]
}
