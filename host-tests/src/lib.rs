//! Host 侧单测：`#[path]` 原样引入嵌入式模块，测的就是烧进板子的代码。
//! 新增被测模块时在这里加一行 `#[path]` 即可。
#![cfg(test)]

#[path = "../../src/data/countdown.rs"]
mod countdown;

use countdown::{
    CountDownItem, Unit, build_items, day_remaining_secs, release_remaining_secs,
    release_total_secs, week_remaining_secs, year_remaining_secs,
};
use time::{Date, Month, OffsetDateTime, Time, UtcOffset, Weekday};

/// 固定 CEST（+02:00），与 `.env` 的 LAODA_TZ_OFFSET 一致
fn tz() -> UtcOffset {
    UtcOffset::from_whole_seconds(7200).unwrap()
}

fn at(y: i32, m: Month, d: u8, h: u8, mi: u8) -> OffsetDateTime {
    Date::from_calendar_date(y, m, d)
        .unwrap()
        .with_time(Time::from_hms(h, mi, 0).unwrap())
        .assume_offset(tz())
}

/// 2026-08-17 是周一（测试日期锚点）
#[test]
fn weekday_anchor() {
    assert_eq!(at(2026, Month::August, 17, 12, 0).weekday(), Weekday::Monday);
}

#[test]
fn day_remaining() {
    // 2026-08-17 周一 / 08-21 周五 / 08-22 周六 / 08-23 周日
    assert_eq!(day_remaining_secs(at(2026, Month::August, 17, 8, 0)), 36_000); // 08:00 → 36000S
    assert_eq!(day_remaining_secs(at(2026, Month::August, 17, 9, 30)), 30_600); // 09:30 → 30600S
    assert_eq!(day_remaining_secs(at(2026, Month::August, 17, 7, 0)), 0); // 08:00 前隐藏
    assert_eq!(day_remaining_secs(at(2026, Month::August, 21, 17, 59)), 60); // 周五 17:59 → 60S
    // 显示单位是秒
    let it = build_items(at(2026, Month::August, 17, 8, 0))[2];
    assert_eq!(it.unit, Unit::Seconds);
    assert_eq!(it.value(), 36_000); // 36000S
    assert_eq!(day_remaining_secs(at(2026, Month::August, 21, 18, 0)), 0); // 周五 18:00 隐藏
    assert_eq!(day_remaining_secs(at(2026, Month::August, 22, 12, 0)), 0); // 周六
    assert_eq!(day_remaining_secs(at(2026, Month::August, 23, 23, 59)), 0); // 周日
}

#[test]
fn week_remaining() {
    assert_eq!(week_remaining_secs(at(2026, Month::August, 17, 8, 0)), 180_000); // 周一 08:00 → 50h
    assert_eq!(week_remaining_secs(at(2026, Month::August, 17, 7, 0)), 180_000); // 周一 08:00 前算完整 10h
    assert_eq!(week_remaining_secs(at(2026, Month::August, 17, 18, 0)), 144_000); // 周一 18:00 → 40h
    assert_eq!(week_remaining_secs(at(2026, Month::August, 19, 12, 0)), 93_600); // 周三 12:00 → 26h
    assert_eq!(week_remaining_secs(at(2026, Month::August, 21, 17, 0)), 3_600); // 周五 17:00 → 1h
    assert_eq!(week_remaining_secs(at(2026, Month::August, 21, 18, 0)), 0); // 周五 18:00 隐藏
    assert_eq!(week_remaining_secs(at(2026, Month::August, 22, 10, 0)), 0); // 周六
    assert_eq!(week_remaining_secs(at(2026, Month::August, 23, 10, 0)), 0); // 周日
    // 显示单位是分钟
    let it = build_items(at(2026, Month::August, 17, 8, 0))[1];
    assert_eq!(it.unit, Unit::Minutes);
    assert_eq!(it.value(), 3000); // 3000M
}

#[test]
fn year_remaining() {
    // 今年剩余工作时间 = 现在到年底的周一~周五 08:00–18:00 秒数之和（不计节假日）
    // 2026 有 261 个工作日、2024（闰年）262 个
    assert_eq!(year_remaining_secs(at(2026, Month::January, 1, 0, 0)), 261 * 36_000);
    assert_eq!(year_remaining_secs(at(2024, Month::January, 1, 0, 0)), 262 * 36_000);
    assert_eq!(year_remaining_secs(at(2026, Month::December, 31, 12, 0)), 6 * 3600); // 周四剩 6h
    assert_eq!(year_remaining_secs(at(2026, Month::December, 31, 18, 0)), 0); // 年底下班 → 0H
    // 显示单位是工作小时：元旦零点回到新整年工作时长、进度条重置
    let it = build_items(at(2027, Month::January, 1, 0, 0))[0];
    assert_eq!(it.value(), 2610); // 261 个工作日 × 10h
    assert_eq!(it.unit, Unit::Hours);
    assert_eq!(it.elapsed(), 0.0);
    assert_eq!(build_items(at(2026, Month::August, 15, 15, 20))[0].value(), 990); // 周六，剩 99 个工作日
    // 2026-08-18 周二 12:42（+02:00 本地）：今日 5h18m + 97 个工作日 → 976H
    assert_eq!(build_items(at(2026, Month::August, 18, 12, 42))[0].value(), 976);
}

#[test]
fn release_remaining() {
    // 起点 2026-01-01 00:00，发布 2027-04-01 18:00（周四）；只算工作时段：326 个工作日
    assert_eq!(release_total_secs(), 326 * 36_000);
    let it = build_items(at(2026, Month::January, 1, 0, 0))[3];
    assert_eq!(it.unit, Unit::Hours);
    assert_eq!(it.value(), 3260); // 3260H
    assert_eq!(it.elapsed(), 0.0); // 起点条为空
    // 周六：只剩发布日前 4 个工作日（3/29 ~ 4/1）
    assert_eq!(release_remaining_secs(at(2027, Month::March, 27, 12, 0)), 40 * 3600);
    assert_eq!(build_items(at(2027, Month::April, 1, 17, 0))[3].value(), 1); // 最后一小时 → 1H
    assert_eq!(release_remaining_secs(at(2027, Month::April, 1, 18, 0)), 0); // 发布时刻 → 0
    assert_eq!(release_remaining_secs(at(2027, Month::April, 2, 0, 0)), 0); // 已过钳 0
    assert_eq!(build_items(at(2027, Month::April, 2, 0, 0))[3].elapsed(), 1.0); // 条画满
}

#[test]
fn visibility_and_rounding() {
    // 周五 17:59：Day 显示 60S；18:00 时 Day/Week 同时隐藏，Year/Release 始终在
    let t = build_items(at(2026, Month::August, 21, 17, 59));
    assert!(t[1].visible() && t[2].visible());
    let t = build_items(at(2026, Month::August, 21, 18, 0));
    assert!(t[0].visible() && !t[1].visible() && !t[2].visible() && t[3].visible());
    // 周末只剩 2 行
    let t = build_items(at(2026, Month::August, 22, 12, 0));
    assert_eq!(t.iter().filter(|it| it.visible()).count(), 2);

    // 向上取整：剩余 1 秒 → 显示 1（最小显示值为 1，不会显示 0 后仍可见）
    let it = CountDownItem {
        label: "T",
        remaining_secs: 1,
        total_secs: 3600,
        unit: Unit::Minutes,
        always_visible: false,
    };
    assert_eq!(it.value(), 1);
    assert!(it.visible());
}
