/// Format a `u8` value into the start of `buf` as digits. Returns the number
/// of bytes written. The caller writes any suffix (e.g. `%`) at that index.
pub fn format_u8(buf: &mut [u8], value: u8) -> usize {
    format_u16(buf, value as u16)
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
