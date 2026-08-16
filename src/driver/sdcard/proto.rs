//! SD 卡 SPI 模式协议里的纯逻辑：命令帧、CRC、CSD 解析。
//!
//! 不碰 esp-hal / embassy，所以 host-tests 里能原样 `#[path]` 引入跑单测。

/// SD 卡固定 512 字节一块（CMD16 会把按字节寻址的老卡也设成这个）
pub const BLOCK_SIZE: usize = 512;

// ---- 命令号（SPI 模式用到的子集）----

/// GO_IDLE_STATE：软复位，进 SPI 模式
pub const CMD0: u8 = 0;
/// SEND_IF_COND：探测 v2 卡与电压范围
pub const CMD8: u8 = 8;
/// SEND_CSD：读容量等卡参数
pub const CMD9: u8 = 9;
/// STOP_TRANSMISSION：结束多块读
pub const CMD12: u8 = 12;
/// SEND_STATUS：读 R2 状态（写入后确认）
pub const CMD13: u8 = 13;
/// SET_BLOCKLEN：按字节寻址的卡上把块长设成 512
pub const CMD16: u8 = 16;
/// READ_SINGLE_BLOCK
pub const CMD17: u8 = 17;
/// READ_MULTIPLE_BLOCK
pub const CMD18: u8 = 18;
/// WRITE_BLOCK
pub const CMD24: u8 = 24;
/// WRITE_MULTIPLE_BLOCK
pub const CMD25: u8 = 25;
/// APP_CMD：下一条是 ACMD
pub const CMD55: u8 = 55;
/// READ_OCR：取 CCS 位判断寻址方式
pub const CMD58: u8 = 58;
/// CRC_ON_OFF
pub const CMD59: u8 = 59;
/// SET_WR_BLK_ERASE_COUNT：多块写前预擦除
pub const ACMD23: u8 = 23;
/// SD_SEND_OP_COND：上电初始化轮询
pub const ACMD41: u8 = 41;

// ---- R1 响应位 ----

/// R1 全 0 = 就绪
pub const R1_READY: u8 = 0x00;
/// 复位后处于 idle
pub const R1_IDLE: u8 = 0x01;
/// 命令不认识（v1 卡对 CMD8 会这样回）
pub const R1_ILLEGAL_COMMAND: u8 = 0x04;

// ---- 数据令牌 ----

/// 单块读写 / 多块读的数据起始令牌
pub const TOKEN_START_BLOCK: u8 = 0xFE;
/// 多块写的数据起始令牌
pub const TOKEN_START_WRITE_MULTI: u8 = 0xFC;
/// 多块写的结束令牌
pub const TOKEN_STOP_TRAN: u8 = 0xFD;
/// 写数据响应令牌的有效位
pub const DATA_RES_MASK: u8 = 0x1F;
/// 写数据被接受
pub const DATA_RES_ACCEPTED: u8 = 0x05;

/// OCR 的 CCS 位（bit 30）：1 = 按块寻址（SDHC/SDXC），0 = 按字节寻址（SDSC）
pub const OCR_CCS: u8 = 0x40; // OCR 最高字节的 bit 6

/// 组命令帧：起始位 01 + 6 位命令号 + 4 字节参数 + CRC7 + 停止位。
///
/// SPI 模式默认不校验 CRC，但 CMD0/CMD8 是例外（此时还没进 SPI 模式），
/// 而且开了 CMD59 之后全都要校验，所以一律算真值。
pub fn command_frame(cmd: u8, arg: u32) -> [u8; 6] {
    let mut frame = [
        0x40 | (cmd & 0x3F),
        (arg >> 24) as u8,
        (arg >> 16) as u8,
        (arg >> 8) as u8,
        arg as u8,
        0,
    ];
    frame[5] = crc7(&frame[..5]);
    frame
}

/// 命令帧用的 CRC7（多项式 x^7+x^3+1），返回值已左移一位并补上停止位
pub fn crc7(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        let mut byte = byte;
        for _ in 0..8 {
            crc <<= 1;
            if ((byte & 0x80) ^ (crc & 0x80)) != 0 {
                crc ^= 0x09;
            }
            byte <<= 1;
        }
    }
    (crc << 1) | 1
}

/// 数据块用的 CRC16（CCITT，多项式 x^16+x^12+x^5+1，初值 0）
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc = crc.rotate_left(8);
        crc ^= u16::from(byte);
        crc ^= (crc & 0xFF) >> 4;
        crc ^= crc << 12;
        crc ^= (crc & 0xFF) << 5;
    }
    crc
}

/// 从 CSD 寄存器里取 `len` 位、最低位在第 `start` 位的字段。
///
/// CSD 是 128 位大端：`csd[0]` 装的是第 127..120 位，所以位号要换算成字节下标。
pub(crate) fn csd_field(csd: &[u8; 16], start: u8, len: u8) -> u32 {
    let mut out = 0u32;
    for i in (0..len).rev() {
        let bit = start + i;
        let byte = 15 - (bit / 8) as usize;
        out = (out << 1) | u32::from((csd[byte] >> (bit % 8)) & 1);
    }
    out
}

/// 从 CSD 算 512 字节块的总数。无法识别的 CSD 版本返回 `None`。
pub fn csd_block_count(csd: &[u8; 16]) -> Option<u32> {
    match csd_field(csd, 126, 2) {
        // CSD v1（SDSC，≤2GB）：容量 = (C_SIZE+1) << (C_SIZE_MULT+2) 个 READ_BL_LEN 大小的块
        0 => {
            let c_size = csd_field(csd, 62, 12);
            let c_size_mult = csd_field(csd, 47, 3);
            let read_bl_len = csd_field(csd, 80, 4);
            // READ_BL_LEN 可能是 9/10/11（512B/1KB/2KB），统一折算成 512B 块
            let blocks_per_unit = 1u32.checked_shl(read_bl_len.checked_sub(9)?)?;
            (c_size + 1)
                .checked_shl(c_size_mult + 2)?
                .checked_mul(blocks_per_unit)
        }
        // CSD v2（SDHC/SDXC）：容量 = (C_SIZE+1) × 512KB
        1 => (csd_field(csd, 48, 22) + 1).checked_mul(1024),
        _ => None,
    }
}
