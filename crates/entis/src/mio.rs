//! MIO sounds ("Entis\x1a" ERISA-Sound files): the sound information record.

/// (channels, samples per second, samples, bits per sample) from the "SoundInf" record.
pub fn info(b: &[u8]) -> Option<(u32, u32, u32, u32)> {
    if !b.starts_with(b"Entis\x1a") {
        return None;
    }
    let i = b.windows(8).position(|w| w == b"SoundInf")?;
    let r = b.get(i + 16..i + 16 + 40)?;
    let u = |k: usize| u32::from_le_bytes(r[k * 4..k * 4 + 4].try_into().unwrap());
    Some((u(3), u(4), u(7), u(9)))
}

/// How long a sound plays (0 when unknown).
pub fn duration_ms(b: &[u8]) -> f64 {
    match info(b) {
        Some((_, rate, n, _)) if rate > 0 => n as f64 * 1000.0 / rate as f64,
        _ => 0.0,
    }
}
