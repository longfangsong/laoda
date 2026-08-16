//! TF 卡（microSD）SPI 模式块驱动，512 字节一块。
//!
//! 板上 TF 卡槽和 LCD 共用 SPI2（MOSI=GPIO6 / SCLK=GPIO7），卡自己的线是
//! MISO=GPIO5、CS=GPIO4。共用带来两个约束，这个驱动直接持总线 mutex 而不是
//! 走 `SpiDevice` 抽象，就是为了满足它们：
//!
//! 1. **CS 必须跨多次传输保持拉低**。一条 SD 命令是「命令帧 → 轮询 R1 →
//!    读/写数据块 → 等忙」，中间要根据卡的回应决定下一步，没法塞进
//!    `SpiDevice::transaction` 的静态 operation 列表；而 `SpiDevice` 的每个方法
//!    都会在结束时抬 CS，把一条命令切成几段，不符合 SD 规范。
//! 2. **速率要换**。初始化阶段必须 ≤400kHz，之后才能提到 [`RUN_FREQ`]，
//!    而 LCD 跑在 80MHz。所以两边都用「每次事务自己设频率」的方式共存：
//!    LCD 侧用 `SpiDeviceWithConfig`，这边用 [`Session::begin`]。
//!
//! 持锁粒度是一次完整的 SD 事务（命令 + 数据 + 等忙）。写入后卡可能忙上百
//! 毫秒，这段时间 LCD 刷新会被挡住，所以文件操作不要放进渲染循环。

pub mod proto;

use aligned::{A4, Aligned};
use block_device_driver::BlockDevice;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::{Mutex, MutexGuard};
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::Async;
use esp_hal::gpio::Output;
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, SpiDmaBus};
use esp_hal::time::Rate;
use log::{debug, info, warn};
use proto::*;

/// 与 LCD 共用的那条 SPI 总线。`NoopRawMutex` 够用：单核 + 所有访问都在同一个
/// executor 里，中断上下文不碰 SPI。
pub type SharedSpiBus = Mutex<NoopRawMutex, SpiDmaBus<'static, Async>>;

/// 一块的类型。`A4` 对齐是为了让 esp-hal 往 DMA 缓冲拷贝时能按字搬。
pub type Block = Aligned<A4, [u8; BLOCK_SIZE]>;

/// 初始化阶段的时钟。规范要求 100–400kHz。
const INIT_FREQ: Rate = Rate::from_khz(400);
/// 初始化完成后的时钟。SPI 模式下卡的上限是 25MHz，留一档余量。
const RUN_FREQ: Rate = Rate::from_mhz(20);

/// 等 R1 / 数据令牌的上限。规范里 NCR ≤ 8 字节、读超时 100ms。
const CMD_TIMEOUT: Duration = Duration::from_millis(500);
/// 等卡释放 DO 的上限。写入 + 内部擦除最坏是几百毫秒。
const BUSY_TIMEOUT: Duration = Duration::from_millis(1000);
/// 上电初始化（CMD0 / ACMD41 轮询）的上限。规范给 ACMD41 的上限是 1s。
const INIT_TIMEOUT: Duration = Duration::from_millis(2000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// SPI 外设报错（DMA 长度、配置非法等）
    Spi,
    /// 卡在超时内没有响应——多半是没插卡
    Timeout,
    /// 认不出来的卡：CMD8 电压回声不对，或不是 SD 卡（MMC 等）
    Unsupported,
    /// 命令的 R1 带错误位
    Command { cmd: u8, r1: u8 },
    /// 读数据块时收到的不是起始令牌（高 3 位为 0 时是错误令牌）
    DataToken(u8),
    /// 写数据块被拒绝
    WriteRejected(u8),
    /// 写入后 CMD13 报告卡内部出错
    WriteFailed { r1: u8, r2: u8 },
    /// 数据块 CRC 不符
    Crc { got: u16, want: u16 },
    /// CSD 版本无法识别，算不出容量
    BadCsd,
    /// 访问越过了卡的末尾
    OutOfRange,
}

impl Error {
    /// 值得重试一次的错误：CRC 不符通常是总线上的偶发干扰，重发一次就好。
    fn retryable(self) -> bool {
        matches!(self, Error::Crc { .. })
    }
}

fn config(freq: Rate) -> SpiConfig {
    SpiConfig::default()
        .with_frequency(freq)
        .with_mode(Mode::_0)
}

/// 一次「CS 拉低到抬起」之间的独占访问：既持有总线锁，也持有 CS。
struct Session<'a> {
    bus: MutexGuard<'a, NoopRawMutex, SpiDmaBus<'static, Async>>,
    cs: &'a mut Output<'static>,
}

impl<'a> Session<'a> {
    async fn begin(
        bus: &'a SharedSpiBus,
        cs: &'a mut Output<'static>,
        freq: Rate,
    ) -> Result<Self, Error> {
        let mut guard = bus.lock().await;
        guard.apply_config(&config(freq)).map_err(|_| Error::Spi)?;
        cs.set_low();
        let mut session = Self { bus: guard, cs };
        // 片选拉低后先空跑一字节，给卡一个建立时间
        session.read_byte().await?;
        Ok(session)
    }

    /// 抬 CS 并再补 8 个时钟——卡要等这一段才会松开 DO。
    async fn end(mut self) {
        self.cs.set_high();
        let _ = self.bus.write_async(&[0xFF]).await;
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        self.bus.write_async(data).await.map_err(|_| Error::Spi)
    }

    /// 全双工收发。注意长度不能超过 DMA 的 rx 缓冲（见 main.rs 的 `dma_buffers!`）。
    async fn transfer(&mut self, data: &mut [u8]) -> Result<(), Error> {
        self.bus
            .transfer_in_place_async(data)
            .await
            .map_err(|_| Error::Spi)
    }

    async fn read_byte(&mut self) -> Result<u8, Error> {
        let mut byte = [0xFF];
        self.transfer(&mut byte).await?;
        Ok(byte[0])
    }

    /// 等卡释放 DO（忙的时候卡把 DO 拉低）。
    ///
    /// 一次读 8 字节而不是 1 字节：每次 DMA 事务都有固定开销，而这里超读
    /// 无害——忙一结束卡就一直吐 0xFF。
    async fn wait_not_busy(&mut self) -> Result<(), Error> {
        with_timeout(BUSY_TIMEOUT, async {
            loop {
                let mut buf = [0xFF; 8];
                self.transfer(&mut buf).await?;
                if buf.contains(&0xFF) {
                    return Ok(());
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    /// 读 R1：卡在 NCR（≤8 字节）内回一个最高位为 0 的字节。
    /// 这里只能一字节一字节读，多读会吃掉紧跟其后的数据。
    async fn response(&mut self) -> Result<u8, Error> {
        // 超时时把 MISO 上最后读到的字节报出来：全 0xFF = 卡根本没应答
        // （没插卡、CS/MISO 接错），全 0x00 = MISO 被拉死。
        let mut last = 0xFFu8;
        let result = with_timeout(CMD_TIMEOUT, async {
            loop {
                let byte = self.read_byte().await?;
                last = byte;
                if byte & 0x80 == 0 {
                    return Ok(byte);
                }
            }
        })
        .await;
        match result {
            Ok(r1) => r1,
            Err(_) => {
                debug!("SD R1 超时，MISO 上最后读到 {:#04X}", last);
                Err(Error::Timeout)
            }
        }
    }

    async fn cmd(&mut self, cmd: u8, arg: u32) -> Result<u8, Error> {
        // CMD0 是复位，卡正忙也照发；其余命令必须等卡空闲
        if cmd != CMD0 {
            self.wait_not_busy().await?;
        }
        self.write(&command_frame(cmd, arg)).await?;
        if cmd == CMD12 {
            // 停止传输的响应前面多一个填充字节
            self.read_byte().await?;
        }
        let r1 = self.response().await;
        debug!("SD CMD{} arg={:#010X} → {:02X?}", cmd, arg, r1);
        r1
    }

    /// 发命令并要求 R1 干净，否则报错
    async fn cmd_ok(&mut self, cmd: u8, arg: u32) -> Result<(), Error> {
        let r1 = self.cmd(cmd, arg).await?;
        if r1 == R1_READY {
            Ok(())
        } else {
            Err(Error::Command { cmd, r1 })
        }
    }

    /// R3（OCR）/ R7（CMD8 回声）：R1 之后还跟 4 个字节
    async fn cmd_with_tail(&mut self, cmd: u8, arg: u32) -> Result<(u8, [u8; 4]), Error> {
        let r1 = self.cmd(cmd, arg).await?;
        let mut tail = [0xFF; 4];
        self.transfer(&mut tail).await?;
        Ok((r1, tail))
    }

    async fn acmd(&mut self, cmd: u8, arg: u32) -> Result<u8, Error> {
        self.cmd(CMD55, 0).await?;
        self.cmd(cmd, arg).await
    }

    /// 收一个数据块：等起始令牌 → 收数据 → 收 2 字节 CRC
    async fn read_data(&mut self, buf: &mut [u8], verify_crc: bool) -> Result<(), Error> {
        let token = with_timeout(CMD_TIMEOUT, async {
            loop {
                let byte = self.read_byte().await?;
                if byte != 0xFF {
                    return Ok::<u8, Error>(byte);
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout)??;
        if token != TOKEN_START_BLOCK {
            return Err(Error::DataToken(token));
        }

        // 收数据期间 MOSI 要保持高电平，所以先把缓冲填 0xFF 再全双工收发
        buf.fill(0xFF);
        self.transfer(buf).await?;

        let mut crc = [0xFF; 2];
        self.transfer(&mut crc).await?;
        if verify_crc {
            let got = u16::from_be_bytes(crc);
            let want = crc16(buf);
            if got != want {
                return Err(Error::Crc { got, want });
            }
        }
        Ok(())
    }

    /// 发一个数据块并确认卡接受了它（不等写完，等忙由调用方负责）
    async fn write_data(&mut self, token: u8, buf: &[u8]) -> Result<(), Error> {
        self.write(&[token]).await?;
        self.write(buf).await?;
        self.write(&crc16(buf).to_be_bytes()).await?;

        // 数据响应令牌紧跟在 CRC 后面，个别卡会先塞一两个 0xFF
        let mut res = 0xFF;
        for _ in 0..8 {
            res = self.read_byte().await?;
            if res != 0xFF {
                break;
            }
        }
        if res & DATA_RES_MASK != DATA_RES_ACCEPTED {
            return Err(Error::WriteRejected(res));
        }
        Ok(())
    }

    /// 写完之后查 R2 状态，确认卡内部没出错（写失败时 R1 是干净的，错在 R2）
    async fn check_write_status(&mut self) -> Result<(), Error> {
        let r1 = self.cmd(CMD13, 0).await?;
        let r2 = self.read_byte().await?;
        if r1 != R1_READY || r2 != 0 {
            return Err(Error::WriteFailed { r1, r2 });
        }
        Ok(())
    }
}

pub struct SdCard {
    bus: &'static SharedSpiBus,
    cs: Output<'static>,
    /// SDHC/SDXC 的地址单位是块，SDSC 是字节
    block_addressing: bool,
    /// 卡上 512B 块的总数（来自 CSD）
    block_count: u32,
    /// CMD59 成功打开 CRC 后才校验读回来的数据
    verify_crc: bool,
}

impl SdCard {
    /// 上电初始化：74 时钟 → CMD0 → CMD8 → CMD59 → ACMD41 → CMD58 → CMD9。
    ///
    /// 没插卡时会在 [`INIT_TIMEOUT`] 后返回 [`Error::Timeout`]，调用方可以据此
    /// 判断卡槽是空的（本机没有卡检测引脚）。
    pub async fn new(bus: &'static SharedSpiBus, mut cs: Output<'static>) -> Result<Self, Error> {
        let mut last_error = Error::Timeout;
        let mut outcome = None;
        // 最多试 3 轮。每轮先在 CS 高电平上打一长串空时钟：规范只要求 74 个，
        // 但这条总线是和 LCD 共用的——卡刚上电时处于 SD 原生模式，那时它不看
        // CS，只听 CMD 线，很容易把屏幕的数据当成垃圾命令而卡住。多打一些时钟
        // 是把卡从这种状态里拽回来的标准手法。
        for attempt in 0..3u8 {
            {
                let mut guard = bus.lock().await;
                guard
                    .apply_config(&config(INIT_FREQ))
                    .map_err(|_| Error::Spi)?;
                cs.set_high();
                guard
                    .write_async(&[0xFF; 128])
                    .await
                    .map_err(|_| Error::Spi)?;
                // CS 高时卡的 DO 是高阻，这里读到的就是总线的静态电平，
                // 和 CS 拉低后的读数一比就知道卡有没有在驱动 DO。
                let mut idle = [0xFF; 8];
                guard
                    .transfer_in_place_async(&mut idle)
                    .await
                    .map_err(|_| Error::Spi)?;
                debug!("SD 第 {} 轮，CS 高电平时 MISO 读到 {:02X?}", attempt, idle);
            }

            let mut session = Session::begin(bus, &mut cs, INIT_FREQ).await?;
            let result = Self::init(&mut session).await;
            session.end().await;
            match result {
                Ok(values) => {
                    outcome = Some(values);
                    break;
                }
                Err(e) => {
                    debug!("SD 第 {} 轮初始化失败：{:?}", attempt, e);
                    last_error = e;
                    Timer::after_millis(50).await;
                }
            }
        }
        let (block_addressing, block_count, verify_crc) = outcome.ok_or(last_error)?;

        let card = Self {
            bus,
            cs,
            block_addressing,
            block_count,
            verify_crc,
        };
        info!(
            "TF 卡就绪：{} MiB，{}寻址，CRC 校验{}",
            card.capacity_bytes() / (1024 * 1024),
            if block_addressing {
                "按块"
            } else {
                "按字节"
            },
            if verify_crc { "开" } else { "关" },
        );
        Ok(card)
    }

    async fn init(session: &mut Session<'_>) -> Result<(bool, u32, bool), Error> {
        // CMD0：进 idle。刚上电的卡可能要试几次才认。
        with_timeout(INIT_TIMEOUT, async {
            loop {
                if session.cmd(CMD0, 0).await? == R1_IDLE {
                    return Ok::<(), Error>(());
                }
                Timer::after_millis(10).await;
            }
        })
        .await
        .map_err(|_| Error::Timeout)??;

        // CMD8：v2 以上的卡会把电压位和校验图样原样回声。
        // 回 illegal command 说明是 v1 卡（≤2GB，按字节寻址）。
        let (r1, tail) = session.cmd_with_tail(CMD8, 0x1AA).await?;
        let v2 = if r1 & R1_ILLEGAL_COMMAND != 0 {
            debug!("TF 卡不认 CMD8，按 v1 卡处理");
            false
        } else {
            if tail[3] != 0xAA {
                return Err(Error::Unsupported);
            }
            true
        };

        // SPI 模式默认不校验 CRC。打开它，代价是每块多算一次 CRC16（~25µs），
        // 换来的是坏数据不会被当好数据用。老卡不支持就算了，照样能跑。
        let verify_crc = matches!(session.cmd(CMD59, 1).await, Ok(R1_IDLE));
        if !verify_crc {
            warn!("TF 卡拒绝 CMD59，数据 CRC 不做校验");
        }

        // ACMD41：轮询到卡完成上电初始化。HCS=1 表示主机认识高容量卡。
        let hcs = if v2 { 1 << 30 } else { 0 };
        with_timeout(INIT_TIMEOUT, async {
            loop {
                if session.acmd(ACMD41, hcs).await? == R1_READY {
                    return Ok::<(), Error>(());
                }
                Timer::after_millis(10).await;
            }
        })
        .await
        .map_err(|_| Error::Timeout)??;

        // CMD58 读 OCR，CCS 位决定地址单位。v1 卡不用问，一定是按字节。
        let block_addressing = if v2 {
            let (r1, ocr) = session.cmd_with_tail(CMD58, 0).await?;
            if r1 != R1_READY {
                return Err(Error::Command { cmd: CMD58, r1 });
            }
            ocr[0] & OCR_CCS != 0
        } else {
            false
        };

        // 按字节寻址的卡块长可变，显式设成 512
        if !block_addressing {
            session.cmd_ok(CMD16, BLOCK_SIZE as u32).await?;
        }

        // CMD9 读 CSD 拿容量
        session.cmd_ok(CMD9, 0).await?;
        let mut csd = [0u8; 16];
        session.read_data(&mut csd, verify_crc).await?;
        let block_count = csd_block_count(&csd).ok_or(Error::BadCsd)?;

        Ok((block_addressing, block_count, verify_crc))
    }

    /// 卡上 512B 块的总数
    pub fn block_count(&self) -> u32 {
        self.block_count
    }

    pub fn capacity_bytes(&self) -> u64 {
        u64::from(self.block_count) * BLOCK_SIZE as u64
    }

    fn address(&self, lba: u32) -> u32 {
        if self.block_addressing {
            lba
        } else {
            lba * BLOCK_SIZE as u32
        }
    }

    fn check_range(&self, lba: u32, count: usize) -> Result<(), Error> {
        let end = u64::from(lba) + count as u64;
        if end > u64::from(self.block_count) {
            return Err(Error::OutOfRange);
        }
        Ok(())
    }

    /// 从 `lba` 起连续读若干块。单块走 CMD17，多块走 CMD18 + CMD12。
    pub async fn read_blocks(&mut self, lba: u32, blocks: &mut [Block]) -> Result<(), Error> {
        if blocks.is_empty() {
            return Ok(());
        }
        self.check_range(lba, blocks.len())?;
        let address = self.address(lba);
        let verify_crc = self.verify_crc;

        // CRC 不符大概率是总线上的偶发干扰（这条总线上 LCD 在跑 80MHz），重试一次
        let mut retried = false;
        loop {
            let mut session = Session::begin(self.bus, &mut self.cs, RUN_FREQ).await?;
            let result = Self::read_inner(&mut session, address, blocks, verify_crc).await;
            session.end().await;
            match result {
                Err(e) if e.retryable() && !retried => {
                    warn!("TF 卡读 LBA {} 出错 {:?}，重试一次", lba, e);
                    retried = true;
                }
                other => return other,
            }
        }
    }

    async fn read_inner(
        session: &mut Session<'_>,
        address: u32,
        blocks: &mut [Block],
        verify_crc: bool,
    ) -> Result<(), Error> {
        if blocks.len() == 1 {
            session.cmd_ok(CMD17, address).await?;
            session.read_data(&mut blocks[0][..], verify_crc).await?;
        } else {
            session.cmd_ok(CMD18, address).await?;
            for block in blocks.iter_mut() {
                session.read_data(&mut block[..], verify_crc).await?;
            }
            session.cmd_ok(CMD12, 0).await?;
        }
        Ok(())
    }

    /// 从 `lba` 起连续写若干块。单块走 CMD24，多块走 CMD25 + 停止令牌。
    pub async fn write_blocks(&mut self, lba: u32, blocks: &[Block]) -> Result<(), Error> {
        if blocks.is_empty() {
            return Ok(());
        }
        self.check_range(lba, blocks.len())?;
        let address = self.address(lba);

        let mut session = Session::begin(self.bus, &mut self.cs, RUN_FREQ).await?;
        let result = Self::write_inner(&mut session, address, blocks).await;
        session.end().await;
        result
    }

    async fn write_inner(
        session: &mut Session<'_>,
        address: u32,
        blocks: &[Block],
    ) -> Result<(), Error> {
        if blocks.len() == 1 {
            session.cmd_ok(CMD24, address).await?;
            session
                .write_data(TOKEN_START_BLOCK, &blocks[0][..])
                .await?;
        } else {
            // ACMD23 提前告诉卡要写几块，卡可以一次擦好，明显更快。
            // 不支持的卡会拒绝，不影响正确性，所以忽略结果。
            let _ = session.acmd(ACMD23, blocks.len() as u32).await;
            session.cmd_ok(CMD25, address).await?;
            for block in blocks {
                session.wait_not_busy().await?;
                session
                    .write_data(TOKEN_START_WRITE_MULTI, &block[..])
                    .await?;
            }
            session.wait_not_busy().await?;
            session.write(&[TOKEN_STOP_TRAN]).await?;
        }
        session.wait_not_busy().await?;
        session.check_write_status().await
    }
}

impl BlockDevice<BLOCK_SIZE> for SdCard {
    type Error = Error;
    type Align = A4;

    async fn read(&mut self, block_address: u32, data: &mut [Block]) -> Result<(), Self::Error> {
        self.read_blocks(block_address, data).await
    }

    async fn write(&mut self, block_address: u32, data: &[Block]) -> Result<(), Self::Error> {
        self.write_blocks(block_address, data).await
    }

    async fn size(&mut self) -> Result<u64, Self::Error> {
        Ok(self.capacity_bytes())
    }
}
