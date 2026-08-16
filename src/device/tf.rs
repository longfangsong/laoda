//! TF 卡文件 IO：块驱动之上挂 FAT（FAT12/16/32），提供文件级读写。
//!
//! 分层：`SdCard`（512B 块）→ `BufStream`（块 ↔ 字节流，内含一块缓存）
//! → `StreamSlice`（把分区框出来）→ `FileSystem`（FAT）。
//!
//! ```rust,ignore
//! let tf = Tf::mount(spi_bus, tf_cs).await?;
//! tf.append("/laoda.log", b"boot\n").await?;
//! let mut buf = [0u8; 64];
//! let n = tf.read("/laoda.log", &mut buf).await?;
//! ```
//!
//! **一致性**：每个写方法结束时都会 flush 到卡（目录项 + FAT + FSInfo），
//! 所以掉电最多丢掉正在写的那一次调用，已返回的调用都已经落盘。代价是每次写
//! 至少多几个块的读改写，别拿它当高频日志用。
//!
//! **并发**：`Tf` 不是 `Sync`，多个任务要用时自己包一层 `embassy_sync::mutex::Mutex`。
//! 一次文件操作会把共用的 SPI 总线持有到操作结束（写入时可能上百毫秒），
//! 期间 LCD 刷新会被挡住，所以别放在渲染循环里。

use alloc::format;
use alloc::string::String;

use block_device_adapters::{BufStream, BufStreamError, StreamSlice, StreamSliceError};
use embedded_fatfs::{
    Date, DateTime, FileSystem, FsOptions, LossyOemCpConverter, Time, TimeProvider,
};
use embedded_io_async::{Read, Seek, SeekFrom, Write};
use embedded_partitions::mbr::{Error as MbrError, Scheme};
use esp_hal::gpio::Output;
use log::{info, warn};

use crate::data::STATE;
use crate::driver::sdcard::proto::BLOCK_SIZE;
use crate::driver::sdcard::{self, SdCard, SharedSpiBus};

/// 卡上的字节流（已经框到某个分区里）
type Storage = StreamSlice<BufStream<SdCard, BLOCK_SIZE>>;
/// 块设备错误穿过 `BufStream` 之后的样子
type BlockError = BufStreamError<sdcard::Error>;
/// `Storage` 的错误类型
pub type IoError = StreamSliceError<BlockError>;
/// 文件操作的错误类型
pub type FsError = embedded_fatfs::Error<IoError>;

/// 挂载失败的原因
#[derive(Debug)]
pub enum MountError {
    /// 卡本身没初始化起来（多半是没插卡）
    Card(sdcard::Error),
    /// 读分区表出错
    Partition(MbrError<BlockError>),
    /// 有分区表，但没有 FAT 分区
    NoFatPartition,
    /// 第一个扇区既不是 MBR 也不是 FAT 引导扇区——没格式化过
    UnknownLayout,
    /// 分区在，但 FAT 结构读不通
    Fs(FsError),
}

/// 文件系统时间戳的来源：NTP 对上时用真实时间，没对上时退回 FAT 纪元 1980-01-01。
///
/// 这样卡里的文件不会带一个骗人的时间——1980 一眼就能看出是「当时还没对时」。
#[derive(Debug, Clone, Copy)]
struct ClockTimeProvider;

impl TimeProvider for ClockTimeProvider {
    fn get_current_date(&self) -> Date {
        self.get_current_date_time().date
    }

    fn get_current_date_time(&self) -> DateTime {
        /// FAT 纪元。没对时的时候用它，一眼能看出「这个时间戳不作数」。
        fn epoch() -> DateTime {
            DateTime::new(Date::new(1980, 1, 1), Time::new(0, 0, 0, 0))
        }

        let Some(clock) = STATE.anon_receiver().try_get().and_then(|s| s.clock) else {
            return epoch();
        };
        let now = clock.now_local();
        // FAT 的年份只能表示 1980–2107，`Date::new` 越界会 panic
        let year = now.year();
        if !(1980..=2107).contains(&year) {
            return epoch();
        }
        DateTime::new(
            Date::new(year as u16, u8::from(now.month()).into(), now.day().into()),
            Time::new(
                now.hour().into(),
                now.minute().into(),
                now.second().into(),
                0,
            ),
        )
    }
}

pub struct Tf {
    fs: FileSystem<Storage, ClockTimeProvider, LossyOemCpConverter>,
}

impl Tf {
    /// 初始化卡并挂载第一个 FAT 分区。
    ///
    /// 分区布局自动识别：常见的 SD 卡是 MBR + 一个 FAT 分区，也支持直接把整卡
    /// 格式化成 FAT（superfloppy，没有分区表）。
    pub async fn mount(
        bus: &'static SharedSpiBus,
        cs: Output<'static>,
    ) -> Result<Self, MountError> {
        let card = SdCard::new(bus, cs).await.map_err(MountError::Card)?;
        let capacity = card.capacity_bytes();

        let stream = BufStream::<_, BLOCK_SIZE>::new(card);
        let storage = match Scheme::open(stream).await.map_err(MountError::Partition)? {
            Scheme::Mbr(mbr) => {
                let index = mbr
                    .iter_used()
                    .find(|(_, p)| p.is_fat())
                    .map(|(i, _)| i)
                    .ok_or(MountError::NoFatPartition)?;
                let entry = mbr.partition(index).expect("index 来自 iter_used");
                info!(
                    "TF 卡分区 {}：{}，起始 LBA {}，{} 个扇区",
                    index,
                    entry.partition_type().name(),
                    entry.start_lba(),
                    entry.sector_count(),
                );
                mbr.into_partition(index)
                    .await
                    .map_err(MountError::Partition)?
            }
            Scheme::Superfloppy(stream) => {
                info!("TF 卡没有分区表，整卡当作一个 FAT 卷");
                StreamSlice::new(stream, 0, capacity)
                    .await
                    .map_err(|e| MountError::Fs(embedded_fatfs::Error::Io(e)))?
            }
            Scheme::Unknown(_) => return Err(MountError::UnknownLayout),
        };

        let fs = FileSystem::new(storage, FsOptions::new().time_provider(ClockTimeProvider))
            .await
            .map_err(MountError::Fs)?;
        info!(
            "TF 卡已挂载：{:?}，簇 {} 字节",
            fs.fat_type(),
            fs.cluster_size()
        );
        Ok(Self { fs })
    }

    /// 读整个文件，最多填满 `buf`，返回读到的字节数。
    /// 文件比 `buf` 长时只读前面一段——需要知道完整长度用 [`Tf::size_of`]。
    pub async fn read(&self, path: &str, buf: &mut [u8]) -> Result<usize, FsError> {
        let mut file = self.fs.root_dir().open_file(path).await?;
        let mut filled = 0;
        while filled < buf.len() {
            let n = file.read(&mut buf[filled..]).await?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        Ok(filled)
    }

    /// 覆盖写：文件不存在就建，存在就先截断。
    pub async fn write(&self, path: &str, data: &[u8]) -> Result<(), FsError> {
        let mut file = self.fs.root_dir().create_file(path).await?;
        file.truncate().await?;
        file.write_all(data).await?;
        file.flush().await?;
        self.fs.flush().await
    }

    /// 追加写：文件不存在就建。日志用这个。
    pub async fn append(&self, path: &str, data: &[u8]) -> Result<(), FsError> {
        let mut file = self.fs.root_dir().create_file(path).await?;
        file.seek(SeekFrom::End(0)).await?;
        file.write_all(data).await?;
        file.flush().await?;
        self.fs.flush().await
    }

    pub async fn exists(&self, path: &str) -> Result<bool, FsError> {
        self.fs.root_dir().exists(path).await
    }

    /// 文件字节数
    pub async fn size_of(&self, path: &str) -> Result<u64, FsError> {
        Ok(self.fs.root_dir().open_meta(path).await?.len())
    }

    /// 删除文件或空目录
    pub async fn remove(&self, path: &str) -> Result<(), FsError> {
        self.fs.root_dir().remove(path).await?;
        self.fs.flush().await
    }

    /// 建目录（路径里的中间层级会一并建出来）
    pub async fn create_dir(&self, path: &str) -> Result<(), FsError> {
        self.fs.root_dir().create_dir(path).await?;
        self.fs.flush().await
    }

    /// 遍历目录，对每个条目回调 `(名字, 是否目录, 字节数)`。
    ///
    /// 用回调而不是返回 `Vec`：条目数不可控，别让它按目录大小吃堆。
    /// `path` 传 `""` 或 `"/"` 表示根目录。
    pub async fn list(
        &self,
        path: &str,
        mut visit: impl FnMut(&str, bool, u64),
    ) -> Result<(), FsError> {
        let root = self.fs.root_dir();
        let dir = if path.is_empty() || path == "/" {
            root
        } else {
            root.open_dir(path).await?
        };
        let mut iter = dir.iter();
        while let Some(entry) = iter.next().await {
            let entry = entry?;
            let name: String = entry.file_name();
            visit(&name, entry.is_dir(), entry.len());
        }
        Ok(())
    }

    /// 卷剩余空间（字节）。要扫 FAT 表，别频繁调用。
    pub async fn free_bytes(&self) -> Result<u64, FsError> {
        let stats = self.fs.stats().await?;
        Ok(u64::from(stats.free_clusters()) * u64::from(stats.cluster_size()))
    }

    /// 卸载：把还没落盘的元数据写下去。正常关机路径应该调它。
    pub async fn unmount(self) -> Result<(), FsError> {
        self.fs.unmount().await
    }

    /// 追加一行开机记录，顺带把「写 + 读元数据」两条路径在真机上跑通一次。
    ///
    /// 这时候 NTP 还没对上，文件时间戳会是 1980（见 [`ClockTimeProvider`]）。
    pub async fn write_boot_record(&self) {
        const RECORD: &[u8] = b"boot\n";
        const PATH: &str = "boot.log";
        if let Err(e) = self.append(PATH, RECORD).await {
            warn!("TF 卡写 {} 失败：{:?}", PATH, e);
            return;
        }
        match self.size_of(PATH).await {
            Ok(size) => info!("TF 卡 {}：第 {} 次开机", PATH, size / RECORD.len() as u64),
            Err(e) => warn!("TF 卡读 {} 失败：{:?}", PATH, e),
        }
    }

    // ------------------------------------------------------------------
    // 临时：真机验证用的自检，验证通过后连同 main.rs 里的调用一起删掉。
    // ------------------------------------------------------------------

    /// 列根目录 → 读第一个普通文件的开头 → 新建一个文件写入并读回校验。
    pub async fn selftest(&self) {
        info!("=== TF 卡自检开始 ===");

        // 1. 列根目录，顺手记下第一个普通文件的名字
        let mut first_file: Option<String> = None;
        let listed = self
            .list("/", |name, is_dir, len| {
                info!("  {} {:>10} {}", if is_dir { "d" } else { "-" }, len, name);
                if !is_dir && first_file.is_none() && len > 0 {
                    first_file = Some(String::from(name));
                }
            })
            .await;
        if let Err(e) = listed {
            warn!("TF 卡列目录失败：{:?}", e);
            return;
        }

        // 2. 读第一个文件的开头。文本按 UTF-8 打印，二进制退化成十六进制。
        if let Some(name) = &first_file {
            let mut buf = [0u8; 128];
            match self.read(name, &mut buf).await {
                Ok(n) => match core::str::from_utf8(&buf[..n]) {
                    Ok(text) => info!("读 {}（前 {} 字节）：{:?}", name, n, text),
                    Err(_) => info!("读 {}（前 {} 字节，二进制）：{:02X?}", name, n, &buf[..n]),
                },
                Err(e) => warn!("读 {} 失败：{:?}", name, e),
            }
        } else {
            info!("根目录下没有普通文件，跳过读测试");
        }

        // 3. 新建文件 → 写 → 读回来比对
        const TEST_PATH: &str = "laoda_selftest.txt";
        let payload = format!(
            "laoda selftest, uptime {} ms\n",
            embassy_time::Instant::now().as_millis()
        );
        if let Err(e) = self.write(TEST_PATH, payload.as_bytes()).await {
            warn!("写 {} 失败：{:?}", TEST_PATH, e);
            return;
        }
        let mut back = [0u8; 64];
        match self.read(TEST_PATH, &mut back).await {
            Ok(n) if &back[..n] == payload.as_bytes() => {
                info!("写读回一致：{} = {:?}", TEST_PATH, payload.trim_end())
            }
            Ok(n) => warn!(
                "读回内容不一致：写入 {} 字节，读回 {} 字节 {:?}",
                payload.len(),
                n,
                core::str::from_utf8(&back[..n])
            ),
            Err(e) => warn!("读回 {} 失败：{:?}", TEST_PATH, e),
        }

        match self.free_bytes().await {
            Ok(free) => info!("=== TF 卡自检结束，剩余空间 {} MiB ===", free / 1024 / 1024),
            Err(e) => warn!("读剩余空间失败：{:?}", e),
        }
    }
}

/// 挂载一次并把失败降级成日志——卡是可选外设，没插卡不该拖垮整机。
pub async fn mount_optional(bus: &'static SharedSpiBus, cs: Output<'static>) -> Option<Tf> {
    match Tf::mount(bus, cs).await {
        Ok(tf) => Some(tf),
        Err(MountError::Card(sdcard::Error::Timeout)) => {
            // 板上没有卡检测引脚，「一直没响应」就是能拿到的最接近「没插卡」的信号。
            // 另一个常见原因是卡本身不支持 SPI 模式——SD 规范 7.0 起 SPI 模式是可选的，
            // 真机上遇到过一张大容量卡从头到尾不回 R1（见设计文档 §17）。
            info!("TF 卡无响应（卡槽为空、没插到位，或该卡不支持 SPI 模式），跳过");
            None
        }
        Err(e) => {
            warn!("TF 卡挂载失败：{:?}", e);
            None
        }
    }
}
