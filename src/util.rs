/// Format a `u8` value into the start of `buf` as digits. Returns the number
/// of bytes written. The caller writes any suffix (e.g. `%`) at that index.
pub fn format_u8(buf: &mut [u8], value: u8) -> usize {
    format_u16(buf, value as u16)
}

/// 解析一条用量推送行（设计文档 §8）。
///
/// 格式：`laoda1 <token> <session> <week> <fable> <epoch>\n`，定长文本行。
/// 返回 `(三个百分比 0..=100, 工作机 epoch 秒)`；任意字段非法返回 `None`。
/// token 为空时任何推送都不匹配（等于禁用推送）。
pub fn parse_usage_push(line: &str, token: &str) -> Option<([u8; 3], u64)> {
    let mut fields: [&str; 6] = [""; 6];
    let mut n = 0;
    for f in line.split_ascii_whitespace() {
        if n == 6 {
            return None; // 字段太多
        }
        fields[n] = f;
        n += 1;
    }
    if n != 6 || fields[0] != "laoda1" || fields[1] != token {
        return None;
    }
    let mut usage = [0u8; 3];
    for (out, src) in usage.iter_mut().zip(&fields[2..5]) {
        let v: u32 = src.parse().ok()?;
        if v > 100 {
            return None;
        }
        *out = v as u8;
    }
    let epoch: u64 = fields[5].parse().ok()?;
    Some((usage, epoch))
}

/// Format a `u16` value into the start of `buf` as digits. Returns the number
/// of bytes written. The caller writes any suffix (e.g. `D`) at that index.
pub fn format_u16(buf: &mut [u8], value: u16) -> usize {
    let digit_count = if value == 0 {
        1
    } else {
        ((value as u32).ilog10() + 1) as usize
    };

    // Write digits most-significant first using a divisor.
    let mut n = value as u32;
    let mut div = 10u32.pow(digit_count as u32 - 1);
    for slot in buf.iter_mut().take(digit_count) {
        *slot = b'0' + (n / div) as u8;
        n %= div;
        div /= 10;
    }

    digit_count
}
