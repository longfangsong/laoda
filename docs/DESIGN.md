# laoda — PRD 与设计文档

> 桌面信息屏固件。硬件：Waveshare ESP32-C6-LCD-1.47（320×172 横屏，ST7789，板载 WS2812 与 BOOT 键）。
>
> 状态：设计阶段，尚未实现。本文是实现的依据，实现过程中如有偏离请同步更新本文。

---

# 第一部分：PRD

## 1. 背景与目标

一块常驻桌面、插电运行的小屏，扫一眼即可获知两件事：

- **还剩多少时间** —— 今天的工作时间、本工作周的工作时间、今年、以及一个手工设定的发布日。
- **Claude 额度用了多少** —— Session / Week / Fable 三个配额的百分比。

设备本身不联网抓取用量（那份数据只存在于运行 Claude Code 的工作机上），由工作机主动推送。**工作机可能关机**，此时设备显示上一次收到的值并标记为过期，其余功能不受影响。

## 2. 术语与时间定义

| 术语 | 定义 |
|---|---|
| 本地时区 | 固定 UTC+8，不处理夏令时 |
| 工作日 | 周一至周五。**不考虑法定节假日与调休** |
| 工作时段 | 每个工作日 08:00–18:00 本地时间，共 10 小时 = 600 分钟 = 36000 秒 |
| 工作周 | 周一 08:00 至周五 18:00，共 5 × 10h = 50 小时 = 180000 秒 |
| 新鲜 (Fresh) | 距上次收到用量推送 ≤ 15 分钟 |
| 过期 (Stale) | 距上次收到用量推送 > 15 分钟 |
| 未知 (Unknown) | 开机以来从未收到过用量推送 |

## 3. 页面一：倒计时

左侧日历（103×103，显示当前月/日），右侧一列条目。**条目数量随时间动态变化，为 2 至 4 条。**

### 3.1 条目定义

| 条目 | 数值 | 单位 | 进度条总量 | 可见性 |
|---|---|---|---|---|
| `Year` | 今年剩余工作时间 | `H` | 今年全年工作时间（工作日数 × 10h） | 始终 |
| `Week` | 本工作周剩余工作分钟 | `M` | 3000min（50h） | 剩余 > 0 时 |
| `Day` | 今日剩余工作秒数 | `S` | 36000s（10h） | 剩余 > 0 时 |
| `SW Release` | 距发布日剩余工作时间 | `H` | 从起点到发布时刻的全体工作时间（工作日数 × 10h） | 始终 |

`SW Release` 的日期与起点在固件里硬编码为常量。

### 3.2 剩余量的精确定义

**Day** —— 当前时刻处于某个工作日的 08:00–18:00 之间时，等于 18:00 减去当前时刻；否则为 0。

因此：周二 09:30 → 30600 秒（显示 `30600S`）；周二 07:00 → 0（隐藏）；周二 18:00 → 0（隐藏）；周六任意时刻 → 0（隐藏）。

**Week** —— 当前时刻到本周五 18:00 之间，落在工作时段内的秒数之和：

- 周末 → 0
- 否则 = `今日剩余部分` + `本周剩余整工作日数 × 10h`
  - 今日剩余部分：若已过 18:00 → 0；若未到 08:00 → 完整 10h；否则 18:00 − 当前时刻
  - 本周剩余整工作日数 = `5 − 今天是周几(1..5)`

因此：周一 08:00 → 3000min（`3000M`）；周三 12:00 → 6h + 2×10h = 26h = 1560min；周五 17:00 → 60min；周五 18:00 → 0（隐藏）；周日 → 0（隐藏）。

**Year** —— 从当前时刻到今年年底，落在工作时段（周一~周五 08:00–18:00，不计节假日）内的秒数之和，向上取整到天。

**统一的可见性规则：`剩余量 == 0` 的条目隐藏；`Year` 与 `SW Release` 标记为始终可见。** 这条规则同时覆盖了"周末不显示 Week""下班不显示 Day"两个需求，不需要单独判断。

### 3.3 取整

**一律向上取整（ceil）。** 剩余 1 秒显示 `1S`，只有恰好走到边界才归零（并随即隐藏）。向下取整会让 59 秒显示成 `0S`，看起来像已经结束。

`SW Release` 同理，最后一个工作小时显示 `1H`。Year 是工作时间口径，12 月 31 日 18:00 后今年已无剩余工作，显示 `0H`（始终可见，不隐藏）。

### 3.4 布局

- 可见条目整体在右侧栏**垂直居中**。行高不变，仅行数变化。
- 只剩 2 行时（周末 / 下班后）右侧偏空、左侧日历相对突出 —— 接受，这本身就是"非工作时间"的视觉信号。
- 条目顺序固定为 Year → Week → Day → SW Release，隐藏项直接从序列中移除，不留空行。

### 3.5 时间未知时

NTP 尚未同步成功时，日历显示 `--`，四条条目全部显示为 `--` 且进度条为空（此时不套用隐藏规则，保持 4 行占位）。

## 4. 页面二：Claude 用量

三个仪表盘横向均分，标签固定为 `Session` / `Week` / `Fable`（标签写死在固件里，不走网络）。数值 0–100 整数。

- **Unknown**（从未收到推送）：仪表显示 `--`，不显示 0%。显示 0% 会被误读成"额度没用"。
- **Stale**：仪表用弱化配色（新增 `theme::TEXT_MUTED`）绘制。
- **Fresh**：正常配色。

## 5. 交互

- 两页每 5 秒自动轮换。
- 按一次 BOOT 键立即切页，并重置自动轮换计时。
- 按键同时把背光恢复到全亮。

## 6. 状态指示（板载 WS2812）

现有的彩虹循环改为状态指示，只在状态变化时更新：

| 颜色 | 含义 |
|---|---|
| 蓝（呼吸/常亮） | WiFi 连接中 |
| 绿 | 在线且用量数据新鲜 |
| 琥珀 | 在线但用量数据过期 |
| 红 | WiFi 断开 |

亮度压到较低档位（约 1/8），避免夜间刺眼且省电。

## 7. 功耗

要求：**尽可能省电，但不熄屏。**

- 背光：默认全亮；无按键操作 60 秒后调暗到约 20%；按键立即恢复全亮。**任何时候都不关闭背光。**
- 屏幕仅在内容真正变化时重绘（详见设计文档 §11）。
- WiFi 启用 modem sleep。

## 8. 非目标

明确不做：

- 法定节假日 / 调休日历
- 设备端配网界面（WiFi 凭据编译期写入）
- 用量数据持久化（重启后到下一次推送之前显示 Unknown，最长空窗 5 分钟）
- 公网访问 / TLS / 出门可用
- 多用户、多设备
- 触摸或多按键交互

## 9. 验收清单

- [ ] 冷启动 → WiFi 连上 → NTP 同步成功后，日历与四条倒计时显示正确值
- [ ] 断开工作机 20 分钟，用量仪表切到 Stale 配色，倒计时与日历不受影响
- [ ] 重启设备，用量显示 `--` 而非 0%，收到第一次推送后恢复
- [ ] 工作日 17:59 → 18:00 跨越时，`Day` 行消失，其余行重新垂直居中
- [ ] 周五 18:00 跨越时，`Week` 与 `Day` 同时消失，只剩 2 行
- [ ] 拔掉路由器（WiFi 断开）后设备不崩溃、不重启，LED 转红，恢复后自动重连
- [ ] 空闲 60 秒后背光变暗，按键后恢复
- [ ] 空闲时（无数据变化、无按键）不发生逐帧重绘

---

# 第二部分：设计文档

## 1. 架构总览

```
┌─ 工作机（可能关机）────────┐
│ cron 每 5 min             │
│  claude -p "/usage"       │
│  → 解析 → 3 个 u8         │
│  → UDP laoda.local        │
└───────────┬───────────────┘
            │ UDP :5005（局域网明文 + token）
            ▼
┌─ ESP32-C6 ────────────────────────────────────────┐
│                                                    │
│  wifi_task ──┐                                     │
│  net_task ───┤                                     │
│  push_task ──┼──▶ STATE: Watch<AppState> ──┐       │
│  sntp_task ──┘                              │       │
│                                             ▼       │
│                                     render loop     │
│                        （事件驱动：状态变化/按键/    │
│                          下一个显示边界/自动切页）   │
│                                             │       │
│  led_task ◀─────────────────────────────────┤       │
│  lcd refresh_task ◀── FrameBuffer ◀─────────┘       │
└────────────────────────────────────────────────────┘
            │
            ▼ UDP 出站
      pool.ntp.org（每 6 小时）
```

两条数据链完全独立：**时间来自 NTP，用量来自推送**。任一失效不影响另一条。

## 2. 模块划分

```
src/
  data/
    mod.rs          AppState、STATE、Freshness、Clock
    clock.rs        基于 Instant 的本地时钟
    countdown.rs    从时间算出可见条目（纯函数，可在 host 上单测）
  net/
    mod.rs
    wifi.rs         连接 / 重连退避 / 状态上报
    stack.rs        embassy-net 初始化与 net_task
    sntp.rs         SNTP 客户端
    push.rs         UDP 监听、token 校验、ack
  ui/               现有结构不变，page 改为纯渲染
  device/           不变
  driver/           不变
  util.rs           数字/时长格式化
```

现有 `ui/` 与 `driver/` 基本不动，`device/lcd.rs` 只改刷新触发方式。

## 3. 日期时间库选型

### 结论：用 `time` crate，`default-features = false`

```toml
time = { version = "0.3", default-features = false }
```

已验证 `time` 0.3.55 的 `std` 是**可选** feature，因此 `default-features = false` 后可在 `no_std` 环境使用；`alloc` / `formatting` / `parsing` / `macros` / `serde` 全部保持关闭。

需要用到的 API 都在核心部分：

```rust
use time::{Date, Duration, Month, OffsetDateTime, Time, UtcOffset, Weekday};

const TZ: UtcOffset = UtcOffset::from_hms(8, 0, 0).unwrap(); // const fn
let now = OffsetDateTime::from_unix_timestamp(epoch)?.to_offset(TZ);

now.weekday()                       // Weekday，含 number_from_monday()
now.date();  now.time();            // Date / Time
now.date().year();  now.date().month();  now.date().day();
Date::from_calendar_date(y, Month::January, 1)?   // 用于算年末、发布日
date_a - date_b                     // Duration
now + Duration::hours(3)
```

### 为什么不自己写

自己写只需要 Hinnant 的 `days_from_civil` / `civil_from_days` 两个函数（各约 15 行）就能覆盖"今年剩余天数"。但新需求引入了**星期判断、工作时段边界、跨日累加**，需要的就不止两个函数了：闰年、月内天数、weekday、时刻比较、时区偏移加减 —— 自己写五六个函数并保证边界正确，不如引一个被广泛使用、测试充分的库。

### 代价与退路

代价是 flash 占用。`time` 的核心部分不算大，但具体数字要实测（`cargo size` 或 `cargo bloat` 对比引入前后）。**这是个可逆决策**：`countdown.rs` 里对 `time` 的使用集中在几个纯函数中，如果实测发现膨胀不可接受，换回手写 Hinnant 只需改这一个文件。

### 备选（不推荐，记录理由）

- **`chrono`**（`default-features = false`）：也支持 no_std，但 no_std 路径上的 API 表面比 `time` 更别扭（很多便利方法依赖 `alloc` 或 `std`），且体积更大。
- **`jiff`**：较新，API 设计更好，但它的 no_std 支持我未做验证。等这个项目稳定后可以再评估。
- **手写**：见上。作为退路保留。

## 4. 本地时钟

不使用 RTC，靠一个基准点 + 单调时钟推算：

```rust
#[derive(Clone, Copy)]
pub struct Clock {
    epoch_at_ref: u64,     // 基准时刻的 Unix 秒
    instant_ref: Instant,  // 取得基准时的单调时刻
}

impl Clock {
    pub fn now_unix(&self) -> u64 {
        self.epoch_at_ref + (Instant::now() - self.instant_ref).as_secs()
    }
    pub fn now_local(&self) -> OffsetDateTime { /* from_unix_timestamp + to_offset(TZ) */ }
}
```

`AppState.clock: Option<Clock>` —— `None` 表示尚未对时。

**同步策略**：开机立即同步；失败按 1s → 2s → 4s → … → 60s 封顶退避重试；成功后每 6 小时重新同步一次。ESP32 晶振精度足够，6 小时的漂移远小于分钟级显示的精度要求。

**冗余时间源**：推送包里也带一个 epoch（工作机的时钟一定是准的，零成本）。仅在 `clock == None` 时采用；NTP 一旦同步成功就以 NTP 为准。这样即使 NTP 服务器不可达，只要工作机开着，时间也是对的。

## 5. 倒计时计算

`data/countdown.rs`，全部为纯函数，输入 `OffsetDateTime`，输出条目数组。**不依赖任何硬件，可以在 host 上写单元测试。**

```rust
const WORK_START: Time = time!(08:00);   // 或 Time::from_hms(8, 0, 0).unwrap()
const WORK_END:   Time = time!(18:00);
const WORK_DAY_SECS:  u32 = 10 * 3600;
const WORK_WEEK_SECS: u32 = 5 * WORK_DAY_SECS;

pub const SW_RELEASE_DATE:  (i32, Month, u8) = (2026, Month::September, 30);
pub const SW_RELEASE_ORIGIN: (i32, Month, u8) = (2026, Month::April, 1);   // 进度条起点

fn day_remaining_secs(now: OffsetDateTime) -> u32 {
    if !is_workday(now.weekday()) { return 0; }
    let t = now.time();
    if t < WORK_START || t >= WORK_END { return 0; }
    (WORK_END - t).whole_seconds() as u32
}

fn week_remaining_secs(now: OffsetDateTime) -> u32 {
    let wd = now.weekday().number_from_monday();   // 周一=1 … 周日=7
    if wd > 5 { return 0; }
    let today = {
        let t = now.time();
        if t >= WORK_END { 0 }
        else if t < WORK_START { WORK_DAY_SECS }
        else { (WORK_END - t).whole_seconds() as u32 }
    };
    today + (5 - wd) as u32 * WORK_DAY_SECS
}
```

注意 `day_remaining_secs` 与 `week_remaining_secs` 中"今日部分"的定义**不同**：前者在 08:00 之前为 0（该行隐藏），后者在 08:00 之前算完整 10 小时（今天的工作还没开始，理应计入本周剩余）。这是刻意的，不是重复代码。

`Year`：从当前时刻到年底，逐日累加落在工作时段（周一~周五 08:00–18:00）内的秒数（今日部分与 Week 同口径），向上取整到小时，以 `H` 显示；进度条总量为全年工作时段秒数之和。

`SW Release`：从当前时刻到发布时刻（发布日 18:00），逐日累加落在工作时段（周一~周五 08:00–18:00）内的秒数，向上取整到小时，以 `H` 显示；已过发布时刻钳 0。进度条总量为起点 00:00 到发布时刻的全体工作时段秒数。

## 6. 数据模型

```rust
// data/mod.rs
#[derive(Clone, Copy, PartialEq)]
pub enum Freshness { Unknown, Fresh, Stale }

#[derive(Clone, Copy, PartialEq)]
pub enum LinkState { Connecting, Online, Offline }

#[derive(Clone, Copy)]
pub struct AppState {
    pub clock:     Option<Clock>,
    pub usage:     [u8; 3],           // 0..=100
    pub usage_at:  Option<Instant>,   // None = 从未收到
    pub link:      LinkState,
}

impl AppState {
    pub fn freshness(&self) -> Freshness { /* usage_at 与 STALE_AFTER 比较 */ }
}

pub static STATE: Watch<CriticalSectionRawMutex, AppState, 2> = Watch::new();
pub const STALE_AFTER: Duration = Duration::from_secs(15 * 60);
```

`AppState` 是 `Copy` 的小结构体，用 `Watch` 而不是 `Mutex + Signal`：多个生产者写最新值，消费者只关心当前值，语义正好吻合。

倒计时条目**不进 AppState**，它是 `clock` 的纯函数，渲染时现算。

## 7. UI 层改动

### 7.1 条目模型

现有 `CountDownItem { label, days_left, total_days }` 表达不了小时和分钟。改为：

```rust
#[derive(Clone, Copy)]
pub enum Unit { Days, Hours, Minutes }

impl Unit {
    const fn secs(self) -> u32 { match self { Days => 86400, Hours => 3600, Minutes => 60 } }
    const fn suffix(self) -> u8 { match self { Days => b'D', Hours => b'H', Minutes => b'M' } }
}

#[derive(Clone, Copy)]
pub struct CountDownItem {
    pub label: &'static str,
    pub remaining_secs: u32,
    pub total_secs: u32,
    pub unit: Unit,
    pub always_visible: bool,
}

impl CountDownItem {
    fn value(&self) -> u32 { self.remaining_secs.div_ceil(self.unit.secs()) }  // 向上取整
    fn elapsed(&self) -> f32 { /* 1 - remaining/total，钳到 0..=1 */ }
    fn visible(&self) -> bool { self.always_visible || self.remaining_secs > 0 }
}
```

### 7.2 动态行数布局

现有常量 `ROWS_TOP` 按固定 4 行居中，改为按可见行数计算：

```rust
const fn rows_top(n: usize) -> i32 {
    (SCREEN_HEIGHT as i32 - ROW_HEIGHT * n as i32 + ROW_GAP) / 2
}
```

`ProgressBar` 目前在 `CountDown::new()` 时以固定原点构造。由于 page 改为纯渲染器，进度条改为**每帧按当前可见序号构造**（值类型，构造开销可忽略）。

### 7.3 page 改为纯渲染器

```rust
// 现在
count_down.set_days_left(i, n);
count_down.draw(&mut *fb)?;

// 改为
CountDown::draw(&mut *fb, &CountDownData { date, items })?;
ClaudeUsage::draw(&mut *fb, &UsageData { values, freshness })?;
```

好处是数据源怎么换都不影响 UI 层，也方便以后在 host 上跑一个模拟器调排版。

### 7.4 主题

新增 `theme::TEXT_MUTED`，用于 Stale 状态的仪表与文字。

### 7.5 字体

`Day` 最大 `36000S`（最长，66px），`Week` 最大 `3000M`（58px），`SW Release` 最大 `3260H`（54px，326 个工作日 × 10h），`Year` 最大 `2620H`（53px，262 个工作日 × 10h）。数值右对齐，标签截断上限按该行数值实测宽度推导（`text_width`）：最长组合 `SW Release`(97px) + `3260H`(54px) + 间距 6 = 157px < 栏宽 169px，`36000S` 所在行标签只有 `Day`(31px)，均不冲突。

## 8. 推送协议

**传输：mDNS 名字 + UDP 单播。** 固件跑 mDNS 应答器（`src/net/mdns.rs`，
hick-embassy），广播 `laoda.local` 主机名与 `_laoda-push._tcp` :5005 服务；
工作机默认 `sendto('laoda.local', 5005)`，设备 bind 5005 被动接收。选它的理由：

- 设备走 DHCP，IP 会变且脚本无法事先知道；mDNS 让同网段任何客户端现查
  当前租约地址，免手工管 IP
- A 记录取自 DHCP 租约：embassy-net 0.9.1 把租约也存进 `static_v4`，
  `Stack::config_v4()` 能读出来（API 文档没写 DHCP 路径，读源码确认）
- 单包无连接，没有 HTTP header 要解析，socket buffer 512 字节足够
- 5 分钟一次的数据，丢包无所谓，下次自然补上

**已知限制**：

- hick 引擎没有"更新记录"API（只有 register/unregister），DHCP 换租约后
  mDNS 会继续广播旧地址直到重启（家庭网络很少发生，可接受）
- 部分公共网络封 mDNS/组播或客户端定向广播（2026-08 实测：关闭组播转发的
  路由器不转发 `255.255.255.255`）。退路：`LAODA_PUSH_ADDR=<设备IP>` 手动
  指定，协议不变。设备 IP 可从串口日志 ack 行的 local_address 读

**载荷：定长文本行**（三个数字，用 JSON 不划算）：

```
laoda1 <token> <session> <week> <fable> <epoch>\n
```

设备侧 `split_ascii_whitespace()` + `parse()`，零依赖。字段超过 5 个再考虑 `serde-json-core`。

**鉴权**：`token` 由 `env!("LAODA_PUSH_TOKEN")` 编译进固件，不匹配直接丢弃。局域网明文，这只是防误触，不是安全边界。

**回执**：设备收到有效包后向来源地址单播一个 `ok\n`。作用有二 —— 工作机能确认推送成功；工作机从回执学到设备 IP，之后可以改用单播。

**已知风险**：部分路由器开启 AP isolation 或过滤定向广播。若广播不通，退路是给设备做 DHCP 静态地址绑定后改用单播 —— **协议不变，只改工作机的目标地址**。实施前需要先在实际网络上验证一次。

## 9. SNTP

不引第三方 crate。SNTP 客户端只需要"发 48 字节、读回包第 40..48 字节的 transmit timestamp"，约 40 行：

```rust
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;  // 1900-01-01 → 1970-01-01

let mut req = [0u8; 48];
req[0] = 0x1B;                                // LI=0, VN=3, Mode=3 (client)
sock.send_to(&req, server).await?;
let (n, _) = with_timeout(TIMEOUT, sock.recv_from(&mut buf)).await??;
let secs = u32::from_be_bytes(buf[40..44].try_into().unwrap()) as u64;
let unix = secs.checked_sub(NTP_UNIX_OFFSET)?;
```

服务器地址通过 embassy-net 的 DNS 解析 `pool.ntp.org`（`socket-dns` feature 已开启）；DNS 失败时回落到硬编码 IP 列表。

## 10. 任务划分

| 任务 | 职责 | 唤醒条件 |
|---|---|---|
| `net_task` | embassy-net runner | 由协议栈驱动 |
| `wifi_task` | 连接、断线重连（指数退避）、写 `LinkState` | 事件驱动 |
| `sntp_task` | 开机同步 + 每 6h 重同步 | 定时 |
| `push_task` | UDP 监听、校验、写 usage、回 ack | 收包 |
| `led_task` | 状态指示灯 | `STATE` 变化 |
| `refresh_task` | framebuffer → SPI DMA | framebuffer 变脏（Signal） |
| main（渲染循环） | 组装数据、调 page 绘制 | 见下 |

渲染循环：

```rust
match select4(
    rx.changed(),                      // 状态变化：推送到达 / 对时成功 / 链路状态变化
    wait_for_press(&mut button),       // 按键
    Timer::at(next_display_boundary),  // 下一个"显示内容会变"的时刻
    Timer::at(next_auto_switch),       // 自动切页
).await { ... }
```

`next_display_boundary` 由当前页面显示的最小时间单位决定：

- 倒计时页且 `Day` 可见 → 下一个整分钟
- 倒计时页且 `Day` 不可见 → 下一个整小时（`Week` 以小时计）
- 两者都不可见 → 下一个本地午夜
- 用量页 → 不需要时间边界（只在 Fresh→Stale 切换时刻需要一次）

## 11. 功耗设计

按收益从大到小：

| 项 | 现状 | 改法 |
|---|---|---|
| 背光 | 常亮满亮度 | 空闲 60s 调暗到 ~20%，按键恢复。**不熄屏** |
| `refresh_task` | 每 16ms 醒来查 `DIRTY` | 改为 `Signal` 驱动，`signal.wait().await`。没有绘制就不醒，CPU 可长时间睡眠 |
| 渲染循环 | 每秒无条件重绘全屏 | 事件驱动（见 §10）。稳态下最快也就每分钟一次 |
| WS2812 | 每 50ms 推进彩虹 | 改为状态指示灯，仅状态变化时更新，亮度降到 ~1/8 |
| WiFi 省电 | 未启用 | `PowerSaveMode::None`。实测 Maximum 下 AP 把发给设备的单播帧缓存到设备下次 DTIM 唤醒才释放，推送延迟数分钟、ack 大量丢失；本机 USB 常电，保持常醒 |
| CPU 主频 | 160MHz | 可降到 80MHz，收益有限，放到最后评估 |

关于 `refresh_task`：现有实现用 `Timer::after` 而非 `Ticker`，注释里解释了这是为了避免饿死绘制方。改成 Signal 驱动后这个问题自动消失 —— 无数据不唤醒，有数据时锁竞争天然公平。

**待确认的小问题**：`Lcd::set_backlight(&mut self, value: u8)` 内部是 `set_duty(value / 4)`。若 esp-hal 的 `Channel::set_duty` 收的是**百分比**（0–100），那么 `set_backlight(255)` 实际只给到 63%，参数命名有误导。做背光调光时一并确认并修正接口语义（建议直接改成收 `percent: u8`）。

## 12. 配置与密钥

`.cargo/config.toml` 会提交进 git，不适合放 WiFi 密码。改为在 `build.rs` 中读取一个 gitignored 的 `.env`：

```rust
for line in std::fs::read_to_string(".env").unwrap_or_default().lines() {
    if let Some((k, v)) = line.split_once('=') {
        println!("cargo:rustc-env={}={}", k.trim(), v.trim());
    }
}
println!("cargo:rerun-if-changed=.env");
```

编译期变量：

| 变量 | 说明 |
|---|---|
| `LAODA_WIFI_SSID` | |
| `LAODA_WIFI_PSK` | |
| `LAODA_PUSH_TOKEN` | 推送鉴权 |
| `LAODA_TZ_OFFSET` | 时区偏移秒数，默认 28800 |

`.gitignore` 加 `.env`；CI 里提供占位值以保证 `cargo check` 通过。

## 13. 对现有代码的改动清单

| 文件 | 改动 |
|---|---|
| `Cargo.toml` | 加 `time`（no default features）。网络相关依赖已齐备，无需新增 |
| `build.rs` | 加 `.env` 读取 |
| `src/lib.rs` | 加 `pub mod data; pub mod net;` |
| `src/bin/main.rs` | 大幅精简：只做外设初始化 + spawn 任务 + 渲染循环。demo 数据推进逻辑全部删除 |
| `src/device/lcd.rs` | `refresh_task` 改为 Signal 驱动；`set_backlight` 语义修正 |
| `src/driver/ws2812.rs` | 不变（`rainbow_task` 从 main 移出并改写为 `led_task`） |
| `src/ui/page/count_down.rs` | `CountDownItem` 换模型；动态行数布局；`draw` 改为纯渲染 |
| `src/ui/page/claude_usage.rs` | `draw` 改为纯渲染；支持 Unknown / Stale 表现 |
| `src/ui/theme.rs` | 加 `TEXT_MUTED` |
| `src/util.rs` | 加时长格式化；`format_u16` 保留 |

## 14. 风险与未决项

| 项 | 影响 | 应对 |
|---|---|---|
| `claude -p "/usage"` 在非交互模式下可能不输出可解析文本 | 阻塞工作机侧实现 | **动手前先手动跑一次确认**。备选：`npx ccusage`、直接解析 `~/.claude/projects/**/*.jsonl`。无论哪种，推给设备的都是同样三个数字，不影响固件设计 |
| mDNS/组播被网络封锁 | `laoda.local` 解析不到 | `LAODA_PUSH_ADDR=<设备IP>` 手动指定，协议不变 |
| `time` crate 体积 | flash 紧张 | 引入前后跑 `cargo bloat` 对比；超预算则退回手写 Hinnant，改动局限在 `countdown.rs` |
| framebuffer 110KB + DMA 16KB + heap 64KB 已占大头 | 网络栈 OOM | 不上 TLS，socket buffer 保持在 512B–1KB 量级 |
| 不处理法定节假日 | 春节期间 `Week` / `Day` 仍会显示 | 已列为非目标。将来若要做，最简方案是让工作机在推送包里带一个"今天是否工作日"的标志位 |

## 15. 实施顺序

按可独立验证的粒度拆分，每步结束都应能烧录运行：

1. **数据层骨架** —— `AppState` / `STATE` / `Clock`，main 里用假数据驱动，验证 Watch 与事件驱动渲染跑通
2. **UI 重构** —— `CountDownItem` 新模型、动态行数、纯渲染 page；仍用假数据，重点验证 2/3/4 行的排版
3. **倒计时算法** —— `countdown.rs` + host 单元测试（覆盖周一早/周三中/周五 17:59/周五 18:00/周末/跨年）
4. **WiFi + 网络栈** —— 连上、拿到 IP、LED 状态指示
5. **SNTP** —— 对时成功后倒计时和日历显示真实值
6. **推送接收** —— UDP 监听 + token + ack；配套写工作机侧 Python 脚本
7. **新鲜度与降级表现** —— Unknown / Stale 配色
8. **功耗优化** —— Signal 驱动刷新、背光调暗、WiFi power save

第 3 步的单元测试需要在 host target 上跑，而 `.cargo/config.toml` 里写死了 `target = "riscv32imac-unknown-none-elf"`。可以 `cargo test --target aarch64-apple-darwin` 显式覆盖，并把 `countdown.rs` 对 `time` 之外的依赖保持为零。

## 16. 实施注记（WiFi + SNTP 落地，2026-02）

第 1、4、5 步已实现，与上文设计的偏差/细化：

- **日期换算用 `time` crate（0.3，no-std，`default-features = false`）**，不自写格里高利日历。
  `OffsetDateTime` 是 Copy，`Clock` 里只存 `epoch_at_ref + Instant`（8 字节）+ 计算出的本地 `OffsetDateTime`。
  时区：`UtcOffset::from_whole_seconds(LAODA_TZ_OFFSET)`，默认 +02:00（CEST，固定夏令时；
  自动切换 CET/CEST 见 docs/backlog.md）。
- **`AppState` / `STATE` 落在 `src/data/mod.rs`**：`embassy_sync::watch::Watch<CriticalSectionRawMutex, AppState, 2>`。
  0.8 的 `send_modify` 闭包参数是 `&mut Option<T>`，包了一层 `data::modify_state()` 消掉噪声。
- **`wifi_task`（`src/net/wifi.rs`）**：未配置凭据时打一次 error 后静默休眠（不刷屏）；断线用
  `wait_for_disconnect_async` 事件驱动重连，无轮询。`PowerSaveMode::Maximum` 在首次连接前设置。
- **`sntp_task`（`src/net/sntp.rs`）**：48 字节报文，LI/VN 不校验，只查 Mode=4、stratum 1..=15、t3 非零。
  `pool.ntp.org` 在瑞典解析到本地池（Telia）；解析失败时依次试 time.cloudflare.com /
  ntp.ubuntu.com / time.google.com 的固定 IPv4（均从瑞典实测可达）。
  超时：DNS 10s、NTP 单发 5s。退避 1s→2s→…封顶 60s，成功固定 6h 重对。解析逻辑经 host 侧 6 例断言校验。
  两个实战踩坑（火车站公共 WiFi 上烧录验证时发现）：
  - **socket 必须绑源端口 123**：Cloudflare/Google 等对临时源端口的请求回 stratum 0 拒服包
    （kiss-o'-death）或干脆不理，池内服务器也普遍过滤临时端口。
  - **stratum 是第 2 字节的完整 8 位**（不是高 2 位）：按 `byte >> 6` 解析会把所有合法响应
    （stratum 1–3）误判为 0 而全部丢弃。
- **`src/net/stack.rs`**：`embassy_net::new` + `StackResources<5>`（DHCP、DNS 查询、NTP、push、mDNS 各占一槽；加任务时忘了腾槽会 panic `adding a socket to a full SocketSet`），
  随机种子固定 `0x51a0_da0d`。`Runner` 由 `net_task` 单独承载（`run()` 返回 `!`）。
  `embassy-net` 需要 `dns` + `multicast` feature（默认均关闭）。
- **`src/net/mdns.rs`**（mDNS 应答，§8）：hick-embassy 0.2（与 embassy-net 0.9 / smoltcp 0.13 配套），
  注册 `laoda.local` + `_laoda-push._tcp` :5005，A 记录取 `config_v4()` 的 DHCP 租约地址；
  `wait_config_up` 后建 socket、bind :5353、join 224.0.0.251 再进 `MdnsState::run`（永不返回）。
  随机数用 30 行 SplitMix64 实现 `rand_core::TryRng`（0.10 对 `Error=Infallible` 有 blanket impl，
  只实现 TryRng 即满足 `Rng`）。1500 字节缓冲用 `StaticCell<MaybeUninit<..>>` 避免大栈帧。
- **渲染侧钩子**：同步成功后 `count_down` 页的日历/星期切到真实日期（`STATE.clock` 有值即真值），
  未同步时维持 2026-07-08 demo 日期。这是上屏验证 SNTP 是否成功的直接手段。
- **CI**：`.env` 缺任何 `LAODA_*` 键时 build.rs 注入空占位，编译与 CI 均不依赖真实凭据；
  WiFi 凭据缺失是运行时问题（日志提示后休眠）。

第 2、3 步已实现（倒计时算法 + UI 纯渲染重构）：

- **`src/data/countdown.rs`**：条目模型（`Unit`/`CountDownItem`，从 `ui/page` 移到数据层，UI 只渲染）
  与全部剩余量纯函数。只依赖 `time`，host 侧单测在 **`host-tests/`**（独立 crate，
  `#[path]` 原样引入被测模块；仓库根 `[build] target` 写死 riscv，所以必须显式
  `host-tests/.cargo/config.toml` 覆盖父目录 riscv 目标，直接 `cargo test` 即可）。
  一个设计确认：Year 显示今年剩余**工作时间**（工作日 08:00–18:00 秒数之和），元旦零点切换到新年全年工作总量（切换点不会为 0）；12 月 31 日 18:00 后为 0H。
- **`ui/page/count_down.rs`**：改为无状态纯渲染器 `draw(&mut fb, &CountDownData)`；
  可见行整体垂直居中（`rows_top(n)`）；进度条每帧按行位置构造。
  未对时（PRD §3.5）：日历与四行全部 `--` 占位（`Calendar::set_date(0, _)`）。
- **`main.rs`**：demo 日期/条目推进逻辑全部删除，每帧从 `STATE.clock` 现算条目。
  用量页仍是 demo 数据（第 6 步 push 落地后替换）。

第 6、7 步已实现（推送接收 + 新鲜度/降级表现）：

- **`src/net/push.rs`**：`push_task` 监听 UDP :5005，`recv_from` 收包、
  [`parse_usage_push`]（纯函数，落在 `src/util.rs`，无 embassy 依赖）解析校验、
  写 `STATE.usage` / `usage_at`、向来源地址单播 `ok\n` 回执。
  token 取自 `option_env!("LAODA_PUSH_TOKEN")`，**为空时静默丢弃所有推送**（只打一次 error）。
  冗余时间源按 §4：仅 `clock == None` 时采用包内 epoch。
- **`claude_usage.rs`**：改为纯渲染器 `ClaudeUsage::draw(&fb, &UsageData { values, freshness })`，
  标签 `Session`/`Week`/`Fable` 写死在页面（不走网络）；仪表盘每帧按数据构造。
  Unknown → 仪表中心 `--`（`Gauge::display` 覆盖文字，不画 0%）；Stale → 填充/数字/标签
  全部用新增的 `theme::TEXT_MUTED`（#58585C）。
- **`main.rs`**：1s 重绘心跳保留（`Day` 以秒计），demo 数据全部删除；
  两页共用同一次 `STATE` 快照。事件驱动的显示边界属第 8 步。
- **工作机侧 `scripts/push_usage.py`**（stdlib only）：跑 `claude -p "/usage"`，
  解析后按 §8 协议发 `laoda.local:5005`（`LAODA_PUSH_ADDR` 可覆盖），等 5s ack，
  退出码 0/1 供 cron 记录；名字解析失败（gaierror）单独提示 mDNS 未生效。
  **风险 #1 已消除**：实测 `claude -p "/usage"`（2.1.233）输出三行可正则提取——
  `Current session: N% used`、`Current week (all models): N% used`、`Current week (Fable): N% used`；
  输出格式随版本变化是唯一脆弱点，脚本解析失败时打印全文并退出 1。
- host 侧单测新增 `parse_usage_push` 两例（合法/边界 + 8 种拒绝路径）。

真机实测（2026-08，Bahnhof_7EAB14 家用路由器，设备 DHCP 地址 192.168.1.232）：

- 早期测量时路由器**组播转发未开**：客户端发起的 `255.255.255.255` 定向广播
  不转发；设备→工作机 ack 单播大部分被丢（6 次中 1 次成功），工作机→设备
  单播稳定可达。ack 丢失原因待 mDNS 版本上电后复测确认（怀疑与组播转发/
  电源管理配置相关，非固件问题——设备日志确认 ack 已发送）。
- 路由器已开启组播转发 + 固件加 mDNS 后，预期 `laoda.local` 解析、推送、
  ack 全链路正常（待复测记录）。
- 设备 IP 的获取途径：macOS 上 `ping laoda.local` / `dns-sd -G ADDR laoda.local`；
  或串口日志 `ack 已发送 → ... local_address: Some(...)` 行；ack 能通时脚本
  成功行直接打印 `设备=<ip>`。

## 17. 实施注记（TF 卡文件 IO，2026-08）

板上 TF 卡槽接在 SPI2 上：**CS=GPIO4、MISO=GPIO5**，MOSI=GPIO6 / SCLK=GPIO7 与 LCD 共用
（Waveshare ESP32-C6-LCD-1.47 wiki）。分三层落地：

```
src/driver/sdcard.rs        SPI 模式 SD 卡块驱动（512B 一块，async）
src/driver/sdcard/proto.rs  命令帧 / CRC7 / CRC16 / CSD 解析（纯逻辑，host 可测）
src/device/tf.rs            挂 FAT，提供文件级 API（Tf）
```

### 为什么块驱动自己写、FAT 用现成的

块驱动这层是**板级**的：要和 LCD 抢同一条总线、要换速率、CS 要跨多次传输保持拉低，
现成 crate（`sdspi`）走的是 `SpiDevice` 抽象，每个方法结束都会抬 CS，和 SD 规范对不上；
自己写反而更短更可控（约 500 行，含注释）。

FAT 这层是**协议**的，读写完整实现（簇分配、FAT 链、目录项、长文件名）上千行且容易写错，
用 `embedded-fatfs`（MabezDev）。它和 `block-device-driver` / `block-device-adapters` /
`embedded-partitions` 同仓库，全部从同一个 commit 取 git 依赖并锁 rev：`embedded-fatfs`
在 crates.io 上只有 0.0.0 占位版，而 `block-device-*` 的已发布版还停在 embedded-io-async 0.6，
跟 0.7 的 `embedded-fatfs` 拼不起来。feature 取 `alloc + lfn + log`，关掉默认的
`chrono`（要 std 时钟）和 `unicode`（只影响非 ASCII 文件名的大小写折叠）。

### 共用总线的两个约束

1. **CS 跨事务**。一条 SD 命令是「命令帧 → 轮询 R1 → 数据块 → 等忙」，中间要看卡的回应
   决定下一步，塞不进 `SpiDevice::transaction` 的静态 operation 列表。所以 `SdCard` 直接
   持 `Mutex<NoopRawMutex, SpiDmaBus>` 和自己的 CS，`Session::begin/end` 界定一次事务。
2. **速率不同**。卡初始化必须 ≤400kHz、之后走 20MHz（SPI 模式上限 25MHz），LCD 是 80MHz。
   两边都改成「每次事务自己设频率」：LCD 的 `LcdSpi` 从 `SpiDevice` 换成
   `SpiDeviceWithConfig`（80MHz），SD 侧在 `Session::begin` 里设。谁也不用管上一个用户留下什么。

代价：一次 SD 事务会把总线独占到结束，写入时卡内部擦除可能占上百毫秒，这段时间 LCD 刷新
被挡住。所以**文件操作不要放进渲染循环**，`Tf` 也不是 `Sync`，多任务用要自己包 mutex。

### 其它落地细节

- **DMA rx 缓冲 64 → 512 字节**。`SpiDmaBus::transfer_in_place_async` 按 tx 缓冲切块、
  把整块长度交给 rx 通道，rx 比一次收发的长度小会直接报 DMA 长度错；SD 一次收发一整块。
  驱动内所有全双工传输都 ≤512 字节，这是个成对的约束。
- **CRC 默认打开**（CMD59）。SPI 模式默认不校验，打开后每块多算一次 CRC16（~25µs，
  相对 512B@20MHz 的 205µs 约 +12%），换来坏数据不会被当好数据用。卡拒绝 CMD59 就退回不校验，
  只打一条 warn。读到 CRC 不符会自动重发一次（同一条总线上 LCD 在跑 80MHz，偶发干扰不算意外）。
- **容量从 CSD 现算**，v1/v2 两种布局都支持（`proto::csd_block_count`）。
- **文件时间戳来自 NTP 时钟**（`ClockTimeProvider` 读 `STATE.clock`）。没对上时用 FAT 纪元
  1980-01-01，一眼能看出「这个时间戳不作数」，而不是编一个假的当前时间。
- **分区布局自动识别**：常见 SD 卡是 MBR + 一个 FAT 分区，也支持整卡格式化成 FAT（superfloppy）。
- **写入后一定 flush**（目录项 + FAT + FSInfo 都落盘），掉电最多丢正在写的那一次调用。
  代价是每次写多几个块的读改写，别拿它当高频日志用。
- **没插卡不影响开机**：`tf::mount_optional` 把失败降级成日志。板上没有卡检测引脚，
  「初始化一直超时」就是最接近「卡槽为空」的信号。
- **内存**（debug 构建实测）：`.bss` 189328 → 193640（+4.3KB，主要是 main 的 future 里
  多了挂载路径），`.stack` 187456 → 182864（两者共享同一段，此消彼长）。
  一次文件写的 future 约 11KB 栈，在 178KB 栈上没问题，但别塞进 arena 很小的任务。
- **host 侧单测**：`proto` 的 CRC 与 CSD 解析共 5 例（含规范里 CMD0=0x95 / CMD8=0x87 两个
  标准帧、真实 SDHC 的 CSD、手工编码的 v1 CSD）。

### 真机实测（2026-08-16）

16GB 卡，MBR + 单个 FAT32 分区（起始 LBA 32，30535424 扇区，簇 8KB）：

```
TF 卡就绪：14910 MiB，按块寻址，CRC 校验开
TF 卡分区 0：Fat32Lba，起始 LBA 32，30535424 个扇区
TF 卡已挂载：Fat32，簇 8192 字节
TF 卡 boot.log：第 3 次开机
读 boot.log（前 15 字节）："boot\nboot\nboot\n"
写读回一致：laoda_selftest.txt = "laoda selftest, uptime 913 ms"
```

列目录、读文件、建文件+写+读回、追加、剩余空间全部通过；`boot.log` 的计数随每次开机递增，
说明追加与目录项更新都真的落盘了。

### 踩坑：**第一张卡根本不支持 SPI 模式**

调了很久才定位。症状：CMD0 永远拿不到 R1，MISO 上是 `0F/1E/3C/78/F0/E1/C3/87`
——全都是 `11110000` 的循环移位，即一个跟 SCLK 严格同步、每字节一个周期的方波，
而且这个状态会一直保持到给卡断电（软复位清不掉）。

排除过程（这些手法下次还能用）：

- 把 MISO 临时接到未接线的 GPIO23 → 读到全 `00`，证明 esp-hal 的读通路没问题，
  波形真实存在于 GPIO5 上。
- 同一条线分别走阻塞和异步 DMA 读 → 结果完全一致，排除 esp-hal 异步路径。
- 量 1024 个空时钟的耗时（3.8ms）→ 确认 `apply_config` 真把频率降到了 400kHz。
- `Flex` 把 CS/MISO 切成纯输入采样：GPIO4 浮空稳定读高 = 卡的 50kΩ 内部上拉在，线是通的；
  GPIO5 空闲时 4000 次采样零跳变 = 没有别的信号源挂在上面。
- mode0/mode3 × 400k/100k 四种组合全试过，一次都没拿到合法 R1。

结论：SPI 模式在 SD 规范 7.0 之后是**可选**的，不少新的大容量卡干脆没实现。
换一张卡即刻正常。所以 `tf::mount_optional` 在超时路径上把这一条也写进了日志。
