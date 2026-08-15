# Backlog

## 自动夏令时切换（Europe/Stockholm）

**背景**：当前 `LAODA_TZ_OFFSET` 是编译期固定偏移，默认 7200（CEST，UTC+2，夏令时）。
冬季 CET（UTC+1）会差一小时，需要手动改 `.env` 重新编译。

**需求**：按日期自动在 CET / CEST 之间切换，无需改配置。

**欧洲夏令时规则（确定性，无歧义，可直接实现）**：

- 开始：每年 3 月最后一个周日，01:00 UTC → CEST（UTC+2）
- 结束：每年 10 月最后一个周日，01:00 UTC → CET（UTC+1）

**实现思路**（任选其一，倾向 a）：

- a. 在 `Clock::now_local()` 里按规则计算当前偏移：给定 Unix 秒 → 年份 →
  找 3 月 / 10 月最后一个周日的 01:00 UTC 边界 → 比较得出 CET/CEST。
  纯整数运算，`time` crate 的 `Date::weekday()` 已可拿到星期；边界换算约 20 行。
  `LAODA_TZ_OFFSET` 保留为"基础偏移"（CET 值），CEST 时 +3600。
- b. `time` crate 的 `TzDatabase`（需启用 tz 数据，no_std 下体积和构建复杂度都上台阶）。

**验收**：跨切换日（3 月 / 10 月最后周日 01:00 UTC）烧录运行，本地时间正确跳变；
host 侧对 `now_local` 加单元断言（切换日前后各取一天 + 切换时刻本身）。
