//! WADY sounds of the Marble engine: 16-bit PCM packed to one byte a sample (music, sounds
//! and some voices), turned into a WAV file. The packing follows GARbro's
//! ArcFormats/Marble/AudioWADY.cs (MIT).
//!
//! ```text
//! "WADY" ?:u8 mul:u8 ?:[u8; 6] size:u32 ?:[u8; 16] WAVEFORMAT (16 bytes) data
//! ```

const SAMPLES: [u16; 128] = [
    0x0000, 0x0002, 0x0004, 0x0006, 0x0008, 0x000A, 0x000C, 0x000F, 0x0012, 0x0015, 0x0018, 0x001C,
    0x0020, 0x0024, 0x0028, 0x002C, 0x0031, 0x0036, 0x003B, 0x0040, 0x0046, 0x004C, 0x0052, 0x0058,
    0x005F, 0x0066, 0x006D, 0x0074, 0x007C, 0x0084, 0x008C, 0x0094, 0x00A0, 0x00AA, 0x00B4, 0x00BE,
    0x00C8, 0x00D2, 0x00DC, 0x00E6, 0x00F0, 0x00FF, 0x010E, 0x011D, 0x012C, 0x0140, 0x0154, 0x0168,
    0x017C, 0x0190, 0x01A9, 0x01C2, 0x01DB, 0x01F4, 0x020D, 0x0226, 0x0244, 0x0262, 0x028A, 0x02BC,
    0x02EE, 0x0320, 0x0384, 0x03E8, 0x0000, 0xFFFE, 0xFFFC, 0xFFFA, 0xFFF8, 0xFFF6, 0xFFF4, 0xFFF1,
    0xFFEE, 0xFFEB, 0xFFE8, 0xFFE4, 0xFFE0, 0xFFDC, 0xFFD8, 0xFFD4, 0xFFCF, 0xFFCA, 0xFFC5, 0xFFC0,
    0xFFBA, 0xFFB4, 0xFFAE, 0xFFA8, 0xFFA1, 0xFF9A, 0xFF93, 0xFF8C, 0xFF84, 0xFF7C, 0xFF74, 0xFF6C,
    0xFF60, 0xFF56, 0xFF4C, 0xFF42, 0xFF38, 0xFF2E, 0xFF24, 0xFF1A, 0xFF10, 0xFF01, 0xFEF2, 0xFEE3,
    0xFED4, 0xFEC0, 0xFEAC, 0xFE98, 0xFE84, 0xFE70, 0xFE57, 0xFE3E, 0xFE25, 0xFE0C, 0xFDF3, 0xFDDA,
    0xFDBC, 0xFD9E, 0xFD76, 0xFD44, 0xFD12, 0xFCE0, 0xFC7C, 0xFC18,
];

const SAMPLES2: [u16; 64] = [
    0x0000, 0x0004, 0x0008, 0x000C, 0x0013, 0x0018, 0x001E, 0x0026, 0x002F, 0x003B, 0x004A, 0x005C,
    0x0073, 0x0090, 0x00B4, 0x00E1, 0x0119, 0x0160, 0x01B8, 0x0226, 0x02AF, 0x035B, 0x0431, 0x053E,
    0x068E, 0x0831, 0x0A3D, 0x0CCD, 0x1000, 0x1400, 0x1900, 0x1F40, 0x0000, 0xFFFC, 0xFFF8, 0xFFF4,
    0xFFED, 0xFFE8, 0xFFE2, 0xFFDA, 0xFFD1, 0xFFC5, 0xFFB6, 0xFFA4, 0xFF8D, 0xFF70, 0xFF4C, 0xFF1F,
    0xFEE7, 0xFEA0, 0xFE48, 0xFDDA, 0xFD51, 0xFCA5, 0xFBCF, 0xFAC2, 0xF972, 0xF7CF, 0xF5C3, 0xF333,
    0xF000, 0xEC00, 0xE700, 0xE0C0,
];

const REPEAT: [usize; 8] = [3, 4, 5, 6, 8, 0x10, 0x20, 0x100];

/// The sound as a WAV file, or `None` when it is not WADY.
pub fn to_wav(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 0x30 || &b[..4] != b"WADY" {
        return None;
    }
    let u16_ = |at: usize| u16::from_le_bytes([b[at], b[at + 1]]);
    let u32_ = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let mul = b[5] as u16;
    let size = u32_(12) as usize;
    let channels = u16_(0x22).max(1) as usize;
    let rate = u32_(0x24);
    let data = &b[0x30..];
    let mut pcm: Vec<i16> = Vec::new();
    if data.len() == size {
        let mut last = [0u16; 2];
        for (i, &v) in data.iter().enumerate() {
            let c = i % channels.min(2);
            last[c] = if v & 0x80 != 0 {
                (v as u16) << 9
            } else {
                last[c].wrapping_add(mul.wrapping_mul(SAMPLES[v as usize]))
            };
            pcm.push(last[c] as i16);
        }
    } else if channels == 1 {
        pcm = unpack(data);
    } else {
        let left_size = u32_(0x30) as usize;
        let left = unpack(&data[4..]);
        let right = unpack(b.get(0x38 + left_size..)?);
        for (k, l) in left.iter().enumerate() {
            pcm.push(*l);
            pcm.push(right.get(k).copied().unwrap_or(0));
        }
    }
    let len = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&(channels as u16).to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    wav.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&len.to_le_bytes());
    for s in pcm {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    Some(wav)
}

/// One channel packed with runs: `unpacked:u32 count:u32 first:i16`, then `count` codes.
fn unpack(b: &[u8]) -> Vec<i16> {
    if b.len() < 10 {
        return Vec::new();
    }
    let count = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let mut sample = i16::from_le_bytes([b[8], b[9]]);
    let mut out = vec![sample];
    let mut at = 10;
    for i in 0..count {
        if i + 300 == count {
            sample = 0;
        }
        let Some(&v) = b.get(at) else { break };
        at += 1;
        if v & 1 != 0 {
            let v = (v >> 1) & 0x7f;
            if v & 0x40 != 0 {
                sample = ((v as u16) << 10) as i16;
            } else {
                sample = sample.wrapping_add(SAMPLES2[v as usize] as i16);
            }
            out.push(sample);
        } else {
            let Some(&hi) = b.get(at) else { break };
            at += 1;
            let v = v as u16 | (hi as u16) << 8;
            let repeat = REPEAT[((v >> 1) & 7) as usize];
            let end = (v & 0xfff0) as i16;
            let inc = (end as f64 - sample as f64) / repeat as f64;
            let mut x = sample as f64;
            for _ in 0..repeat {
                x += inc;
                out.push(x as i16);
            }
            sample = end;
        }
    }
    out
}
