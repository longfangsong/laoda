//! Host 侧单测：`#[path]` 原样引入嵌入式模块，测的就是烧进板子的代码。
//! 新增被测模块时在这里加一行 `#[path]` 即可。
#![cfg(test)]

#[path = "../../src/data/countdown.rs"]
mod countdown;

#[path = "../../src/util.rs"]
mod util;

/// SD 卡协议里的纯逻辑：CRC 与 CSD 解析
#[path = "../../src/driver/sdcard/proto.rs"]
#[allow(dead_code, reason = "命令号等常量给固件用，host 侧只断言其中一部分")]
mod sdcard_proto;

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

// ---- 用量推送行解析（设计文档 §8） ----

#[test]
fn parse_usage_push_valid() {
    let (usage, epoch) =
        util::parse_usage_push("laoda1 tok 42 77 13 1786000000\n", "tok").unwrap();
    assert_eq!(usage, [42, 77, 13]);
    assert_eq!(epoch, 1_786_000_000);
    // 边界值合法；字段间任意空白
    assert_eq!(
        util::parse_usage_push("laoda1 tok   0   100   0  0", "tok").unwrap(),
        ([0u8, 100, 0], 0u64)
    );
}

#[test]
fn parse_usage_push_rejects() {
    let ok = "laoda1 tok 42 77 13 1786000000";
    assert!(util::parse_usage_push(ok, "wrong").is_none()); // token 不匹配
    assert!(util::parse_usage_push(ok, "").is_none()); // token 为空 = 禁用
    assert!(util::parse_usage_push("laoda2 tok 42 77 13 1786000000", "tok").is_none()); // magic
    assert!(util::parse_usage_push("laoda1 tok 42 77 13", "tok").is_none()); // 缺 epoch
    assert!(util::parse_usage_push("laoda1 tok 42 77 13 1786000000 x", "tok").is_none()); // 多字段
    assert!(util::parse_usage_push("laoda1 tok 101 77 13 1786000000", "tok").is_none()); // >100
    assert!(util::parse_usage_push("laoda1 tok 42 abc 13 1786000000", "tok").is_none()); // 非数字
    assert!(util::parse_usage_push("laoda1 tok 42 77 13 -5", "tok").is_none()); // 负 epoch
}

// ---- SD 卡协议（src/driver/sdcard/proto.rs）----

mod sdcard {
    use crate::sdcard_proto::*;

    /// SD 规范里给出的两个标准帧：CMD0 的 CRC 是 0x95，CMD8(0x1AA) 是 0x87
    #[test]
    fn known_command_frames() {
        assert_eq!(command_frame(CMD0, 0), [0x40, 0, 0, 0, 0, 0x95]);
        assert_eq!(command_frame(CMD8, 0x1AA), [0x48, 0, 0, 0x01, 0xAA, 0x87]);
        // 起始位 01 + 6 位命令号
        assert_eq!(command_frame(CMD17, 0x0000_2000)[0], 0x51);
    }

    #[test]
    fn crc16_vectors() {
        assert_eq!(crc16(&[0u8; BLOCK_SIZE]), 0x0000);
        assert_eq!(crc16(&[0xFFu8; BLOCK_SIZE]), 0x7FA1);
        assert_eq!(crc16(&[0x00, 0x01, 0x02, 0x03]), 0x6131);
    }

    /// SDHC 卡的真实 CSD：v2，C_SIZE = 0x00EDC8 = 60872 → 约 32GB
    #[test]
    fn csd_v2_capacity() {
        let csd = [
            0x40, 0x0E, 0x00, 0x32, 0x5B, 0x59, 0x00, 0x00, 0xED, 0xC8, 0x7F, 0x80, 0x0A, 0x40,
            0x40, 0x00,
        ];
        assert_eq!(csd_field(&csd, 126, 2), 1);
        assert_eq!(csd_field(&csd, 48, 22), 60872);
        assert_eq!(csd_block_count(&csd), Some(60_873 * 1024));
    }

    /// 手工编码的 v1 CSD：READ_BL_LEN=9、C_SIZE=3751、C_SIZE_MULT=7
    /// → (3751+1) << 9 = 1_921_024 块 ≈ 983MB
    #[test]
    fn csd_v1_capacity() {
        let csd = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x59, 0x03, 0xA9, 0xC0, 0x03, 0x80, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        assert_eq!(csd_field(&csd, 126, 2), 0);
        assert_eq!(csd_field(&csd, 80, 4), 9);
        assert_eq!(csd_field(&csd, 62, 12), 3751);
        assert_eq!(csd_field(&csd, 47, 3), 7);
        assert_eq!(csd_block_count(&csd), Some(1_921_024));
    }

    /// READ_BL_LEN=10 的老卡：一个单元 1KB，折算成 512B 块要再乘 2
    #[test]
    fn csd_v1_read_bl_len_10() {
        let mut csd = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x5A, 0x03, 0xA9, 0xC0, 0x03, 0x80, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        assert_eq!(csd_field(&csd, 80, 4), 10);
        assert_eq!(csd_block_count(&csd), Some(1_921_024 * 2));
        // CSD v3 及以后没定义，不猜
        csd[0] = 0xC0;
        assert_eq!(csd_block_count(&csd), None);
    }
}
