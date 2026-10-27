//! MIO sounds ("Entis\x1a" ERISA-Sound files): the sound information record and the decoder
//! (lossless ERI and the LOT/DCT codings, with ERINA or gamma run-length codes). Ported from
//! GARbro's MioDecoder and ErisaMatrix (MIT, morkt).

use crate::eri::Ctx;
use crate::erisa::Bits;

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

const LOSSLESS: i32 = 0x0301_0000;
const LOT: i32 = 0x0000_0005;
const LOT_MSS: i32 = 0x0001_0005;
const GAMMA: i32 = -1;
/// MIO uses ERINA codes for the run-length gamma architecture too (the gamma byte coder
/// reads the files short).
const USE_GAMMA: bool = false;
const HUFFMAN: i32 = -4;
const LEAD_BLOCK: u8 = 1;

#[derive(Clone, Copy)]
struct Info {
    transform: i32,
    arch: i32,
    channels: usize,
    rate: u32,
    subband: i32,
    lapped: u32,
    bits: u32,
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The sections (id, contents) of an EriFile record list.
fn sections(b: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 16 <= b.len() {
        let id = &b[at..at + 8];
        let len = le64(b, at + 8).unwrap_or(0) as usize;
        let end = (at + 16).saturating_add(len).min(b.len());
        out.push((id, &b[at + 16..end]));
        at = end;
    }
    out
}

/// Decodes a MIO file to a RIFF WAVE file.
pub fn to_wav(b: &[u8]) -> Option<Vec<u8>> {
    if !b.starts_with(b"Entis\x1a") || b.len() < 0x40 {
        return None;
    }
    let mut info = None;
    let mut chunks = Vec::new();
    for (id, body) in sections(&b[0x40..]) {
        match id {
            b"Header  " => {
                for (id, r) in sections(body) {
                    if id == b"SoundInf" {
                        let u = |k: usize| le32(r, k * 4).unwrap_or(0);
                        info = Some(Info {
                            transform: u(1) as i32,
                            arch: u(2) as i32,
                            channels: u(3) as usize,
                            rate: u(4),
                            subband: u(6) as i32,
                            lapped: u(8),
                            bits: u(9),
                        });
                    }
                }
            }
            b"Stream  " => {
                for (id, c) in sections(body) {
                    if id == b"SoundStm" && c.len() >= 8 {
                        chunks.push((c[1], le32(c, 4)?, &c[8..]));
                    }
                }
            }
            _ => {}
        }
    }
    let info = info?;
    let mut d = Decoder::new(info)?;
    let frame = info.channels * info.bits as usize / 8;
    let total: usize = chunks.iter().map(|c| c.1 as usize).sum();
    let mut pcm = Vec::with_capacity(total * frame);
    let mut st = State::default();
    for (flags, samples, data) in chunks {
        let mut wave = vec![0u8; samples as usize * frame];
        let mut ctx = Ctx {
            bits: Bits::new(data),
            zero: st.zero,
            length: st.length,
            trees: std::mem::take(&mut st.trees),
            last_tree: st.last_tree,
        };
        let ok = d.decode(&mut ctx, flags, samples, &mut wave);
        st = State {
            zero: ctx.zero,
            length: ctx.length,
            trees: std::mem::take(&mut ctx.trees),
            last_tree: ctx.last_tree,
        };
        pcm.extend_from_slice(&wave);
        if !ok {
            break;
        }
    }
    Some(wav(info, &pcm))
}

#[derive(Default)]
struct State {
    zero: i32,
    length: u32,
    trees: Vec<crate::eri::Tree>,
    last_tree: usize,
}

fn wav(info: Info, pcm: &[u8]) -> Vec<u8> {
    let ch = info.channels as u16;
    let bits = info.bits as u16;
    let align = ch * bits / 8;
    let mut w = Vec::with_capacity(pcm.len() + 44);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&ch.to_le_bytes());
    w.extend_from_slice(&info.rate.to_le_bytes());
    w.extend_from_slice(&(info.rate * align as u32).to_le_bytes());
    w.extend_from_slice(&align.to_le_bytes());
    w.extend_from_slice(&bits.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(pcm);
    w
}

struct Decoder {
    info: Info,
    k: Vec<Vec<f32>>,
    buf1: Vec<i32>,
    buf2: Vec<i32>,
    buf3: Vec<i8>,
    division: Vec<u8>,
    revcode: Vec<u8>,
    weight: Vec<i32>,
    coef: Vec<i32>,
    matrix: Vec<f32>,
    internal: Vec<f32>,
    work: Vec<f32>,
    weight_table: Vec<f32>,
    last_dct: Vec<f32>,
    next_rev: usize,
    next_weight: usize,
    next_coef: usize,
    next_src: usize,
    last_dct_buf: usize,
    degree: i32,
    num: usize,
    revolve: Vec<(f32, f32)>,
    freq: [i32; 7],
}

const FREQ_WIDTH: [i32; 7] = [-6, -6, -5, -4, -3, -2, -1];

impl Decoder {
    fn new(info: Info) -> Option<Decoder> {
        if info.channels != 1 && info.channels != 2 {
            return None;
        }
        let mut k = vec![Vec::new()];
        for i in 1..12 {
            let n = 1usize << i;
            let nr = std::f64::consts::PI / (4.0 * n as f64);
            k.push(
                (0..n)
                    .map(|j| (nr + 2.0 * nr * j as f64).cos() as f32)
                    .collect(),
            );
        }
        let mut d = Decoder {
            info,
            k,
            buf1: Vec::new(),
            buf2: Vec::new(),
            buf3: Vec::new(),
            division: Vec::new(),
            revcode: Vec::new(),
            weight: Vec::new(),
            coef: Vec::new(),
            matrix: Vec::new(),
            internal: Vec::new(),
            work: Vec::new(),
            weight_table: Vec::new(),
            last_dct: Vec::new(),
            next_rev: 0,
            next_weight: 0,
            next_coef: 0,
            next_src: 0,
            last_dct_buf: 0,
            degree: 0,
            num: 0,
            revolve: Vec::new(),
            freq: [0; 7],
        };
        if info.transform == LOSSLESS {
            if info.arch != HUFFMAN || (info.bits != 8 && info.bits != 16) {
                return None;
            }
        } else if info.transform == LOT || info.transform == LOT_MSS {
            if info.bits != 16 || !(8..=12).contains(&info.subband) || info.lapped != 1 {
                return None;
            }
            if info.arch != GAMMA && info.arch != HUFFMAN {
                return None;
            }
            let sub = 1usize << info.subband;
            let block = info.channels * sub;
            d.buf1 = vec![0; block];
            d.matrix = vec![0.0; block];
            d.internal = vec![0.0; block];
            d.work = vec![0.0; sub];
            d.weight_table = vec![0.0; sub];
            d.last_dct = vec![0.0; block * info.lapped as usize];
            d.init_degree(info.subband);
        } else {
            return None;
        }
        Some(d)
    }

    fn init_degree(&mut self, degree: i32) {
        self.revolve = revolve_parameter(degree);
        let mut j = 0;
        for i in 0..7 {
            let w = 1 << (degree + FREQ_WIDTH[i]);
            self.freq[i] = j + w / 2;
            j += w;
        }
        self.degree = degree;
        self.num = 1 << degree;
    }

    fn prepare(&self, ctx: &mut Ctx) {
        if USE_GAMMA && self.info.arch == GAMMA {
            ctx.init_gamma();
        } else {
            ctx.prepare_erina();
        }
    }

    fn bytes(&self, ctx: &mut Ctx, dst: &mut [i8]) -> usize {
        if ctx.trees.is_empty() && !(USE_GAMMA && self.info.arch == GAMMA) {
            ctx.prepare_erina();
        }
        if USE_GAMMA && self.info.arch == GAMMA {
            ctx.gamma_bytes(dst)
        } else {
            ctx.erina_bytes(dst)
        }
    }

    fn decode(&mut self, ctx: &mut Ctx, flags: u8, samples: u32, wave: &mut [u8]) -> bool {
        ctx.bits.flush();
        if self.info.transform == LOSSLESS {
            if flags & LEAD_BLOCK != 0 {
                ctx.prepare_erina();
            }
            return if self.info.bits == 8 {
                self.pcm8(ctx, samples, wave)
            } else {
                self.pcm16(ctx, samples, wave)
            };
        }
        if self.info.channels != 2 || self.info.transform == LOT {
            self.dct(ctx, flags, samples, wave)
        } else {
            self.dct_mss(ctx, flags, samples, wave)
        }
    }

    fn pcm8(&mut self, ctx: &mut Ctx, samples: u32, wave: &mut [u8]) -> bool {
        let ch = self.info.channels;
        let n = samples as usize * ch;
        let mut buf = vec![0i8; n];
        if self.bytes(ctx, &mut buf) < n {
            return false;
        }
        let mut src = 0;
        for c in 0..ch {
            let mut v = 0i8;
            for j in 0..samples as usize {
                v = v.wrapping_add(buf[src]);
                src += 1;
                wave[j * ch + c] = v as u8;
            }
        }
        true
    }

    fn pcm16(&mut self, ctx: &mut Ctx, samples: u32, wave: &mut [u8]) -> bool {
        let ch = self.info.channels;
        let s = samples as usize;
        let n = s * ch * 2;
        let mut buf = vec![0i8; n];
        if self.bytes(ctx, &mut buf) < n {
            return false;
        }
        let mut words = vec![0i16; s * ch];
        for c in 0..ch {
            let off = c * s * 2;
            for j in 0..s {
                let low = buf[off + s + j];
                let high = buf[off + j] ^ (low >> 7);
                words[c * s + j] = i16::from_le_bytes([low as u8, high as u8]);
            }
        }
        for c in 0..ch {
            let (mut v, mut d) = (0i16, 0i16);
            for j in 0..s {
                d = d.wrapping_add(words[c * s + j]);
                v = v.wrapping_add(d);
                let at = (j * ch + c) * 2;
                wave[at..at + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        true
    }

    /// Reads the coefficients: high bytes then low bytes, `step` apart per subband.
    fn read_coefficients(&mut self, ctx: &mut Ctx, all: usize, rows: usize, cols: usize) -> bool {
        self.buf3.resize(all * 2, 0);
        let mut b3 = std::mem::take(&mut self.buf3);
        let got = self.bytes(ctx, &mut b3);
        self.buf3 = b3;
        if got < all * 2 {
            return false;
        }
        self.buf2.resize(all, 0);
        let (mut h, mut l) = (0, all);
        for i in 0..rows {
            let mut q = i;
            for _ in 0..cols {
                let low = self.buf3[l] as i32;
                let high = self.buf3[h] as i32 ^ (low >> 8);
                l += 1;
                h += 1;
                self.buf2[q] = (low & 0xff) | (high << 8);
                q += rows;
            }
        }
        true
    }

    fn dct(&mut self, ctx: &mut Ctx, flags: u8, samples: u32, wave: &mut [u8]) -> bool {
        let width = 1u32 << self.info.subband;
        let count = (samples + width - 1) & !(width - 1);
        let subbands = (count >> self.info.subband) as usize;
        let ch = self.info.channels;
        let all = count as usize * ch;
        if ctx.bits.bit() != 0 {
            return false;
        }
        self.division.clear();
        self.weight.clear();
        self.coef.clear();
        let mut last = vec![-1i32; ch];
        for i in 0..subbands {
            for l in last.iter_mut() {
                let code = ctx.bits.bits(2) as i32;
                self.division.push(code as u8);
                if code != *l {
                    if i != 0 {
                        self.weight.push(ctx.bits.bits(32) as i32);
                        self.coef.push(ctx.bits.bits(16) as i32);
                    }
                    *l = code;
                }
                for _ in 0..1 << code {
                    self.weight.push(ctx.bits.bits(32) as i32);
                    self.coef.push(ctx.bits.bits(16) as i32);
                }
            }
        }
        if subbands > 0 {
            for _ in 0..ch {
                self.weight.push(ctx.bits.bits(32) as i32);
                self.coef.push(ctx.bits.bits(16) as i32);
            }
        }
        if ctx.bits.bit() != 0 {
            return false;
        }
        if flags & LEAD_BLOCK != 0 {
            self.prepare(ctx);
        }
        if !self.read_coefficients(ctx, all, width as usize, subbands * ch) {
            return false;
        }
        let mut rest = vec![samples; ch];
        let mut dst: Vec<usize> = (0..ch).map(|c| c * 2).collect();
        let mut next_div = 0;
        self.next_weight = 0;
        self.next_coef = 0;
        self.next_src = 0;
        let mut last = vec![-1i32; ch];
        let mut cur = -1;
        let sub = self.info.subband;
        for i in 0..subbands {
            for j in 0..ch {
                let code = self.division[next_div] as i32;
                next_div += 1;
                self.last_dct_buf = width as usize * self.info.lapped as usize * j;
                let mut lead = false;
                if last[j] != code {
                    if i != 0 {
                        if cur != last[j] {
                            self.init_degree(sub - last[j]);
                            cur = last[j];
                        }
                        let n = rest[j].min(self.num as u32);
                        self.post_block(wave, dst[j], n);
                        rest[j] -= n;
                        dst[j] += n as usize * ch * 2;
                    }
                    last[j] = code;
                    lead = true;
                }
                if cur != code {
                    self.init_degree(sub - code);
                    cur = code;
                }
                for _ in 0..1 << code {
                    if lead {
                        self.lead_block();
                        lead = false;
                    } else {
                        let n = rest[j].min(self.num as u32);
                        self.internal_block(wave, dst[j], n);
                        rest[j] -= n;
                        dst[j] += n as usize * ch * 2;
                    }
                }
            }
        }
        if subbands > 0 {
            for i in 0..ch {
                self.last_dct_buf = width as usize * self.info.lapped as usize * i;
                if cur != last[i] {
                    self.init_degree(sub - last[i]);
                    cur = last[i];
                }
                let n = rest[i].min(self.num as u32);
                self.post_block(wave, dst[i], n);
                rest[i] -= n;
                dst[i] += n as usize * ch * 2;
            }
        }
        true
    }

    fn next_codes(&mut self) -> (i32, i32) {
        let w = self.weight.get(self.next_weight).copied().unwrap_or(0);
        let c = self.coef.get(self.next_coef).copied().unwrap_or(0);
        self.next_weight += 1;
        self.next_coef += 1;
        (w, c)
    }

    fn src(&mut self) -> i32 {
        let v = self.buf2.get(self.next_src).copied().unwrap_or(0);
        self.next_src += 1;
        v
    }

    /// Fills buf1 with zeros at even places and the next coefficients at odd ones.
    fn half_source(&mut self) {
        for i in 0..self.num / 2 {
            self.buf1[i * 2] = 0;
            self.buf1[i * 2 + 1] = self.src();
        }
    }

    fn internal_block(&mut self, wave: &mut [u8], at: usize, n: u32) {
        let (w, c) = self.next_codes();
        let num = self.num;
        let src: Vec<i32> = (0..num).map(|_| self.src()).collect();
        let mut m = std::mem::take(&mut self.matrix);
        self.iquantumize(&mut m[..num], &src, w, c);
        self.decode_tail(&mut m, 0, self.last_dct_buf, true);
        self.matrix = m;
        if n != 0 {
            round_words(wave, at, self.info.channels, &self.internal, n as usize);
        }
    }

    /// ILOT with the lapped buffer at `lap`, keeping this block for the next one (`keep`),
    /// then the IDCT into `internal`.
    fn decode_tail(&mut self, m: &mut [f32], off: usize, lap: usize, keep: bool) {
        let num = self.num;
        let deg = self.degree;
        if keep {
            odd_givens_inverse(&mut m[off..off + num], &self.revolve, deg);
        }
        fast_iplot(&mut m[off..off + num]);
        for i in (0..num).step_by(2) {
            let r1 = self.last_dct[lap + i];
            let r2 = m[off + i + 1];
            self.work[i] = r1 + r2;
            self.work[i + 1] = r1 - r2;
        }
        if keep {
            self.last_dct[lap..lap + num].copy_from_slice(&m[off..off + num]);
        }
        m[off..off + num].copy_from_slice(&self.work[..num]);
        let src = m[off..off + num].to_vec();
        fast_idct(&self.k, &mut self.internal[..num], &src, 0, 1, deg);
    }

    fn lead_block(&mut self) {
        let (w, c) = self.next_codes();
        self.half_source();
        let num = self.num;
        let lap = self.last_dct_buf;
        let src = self.buf1[..num].to_vec();
        let mut l = std::mem::take(&mut self.last_dct);
        self.iquantumize(&mut l[lap..lap + num], &src, w, c);
        odd_givens_inverse(&mut l[lap..lap + num], &self.revolve, self.degree);
        for i in (0..num).step_by(2) {
            l[lap + i] = l[lap + i + 1];
        }
        fast_iplot(&mut l[lap..lap + num]);
        self.last_dct = l;
    }

    fn post_block(&mut self, wave: &mut [u8], at: usize, n: u32) {
        let (w, c) = self.next_codes();
        self.half_source();
        let num = self.num;
        let src = self.buf1[..num].to_vec();
        let mut m = std::mem::take(&mut self.matrix);
        self.iquantumize(&mut m[..num], &src, w, c);
        odd_givens_inverse(&mut m[..num], &self.revolve, self.degree);
        for i in (0..num).step_by(2) {
            m[i] = -m[i + 1];
        }
        self.decode_tail(&mut m, 0, self.last_dct_buf, false);
        self.matrix = m;
        if n != 0 {
            round_words(wave, at, self.info.channels, &self.internal, n as usize);
        }
    }

    fn dct_mss(&mut self, ctx: &mut Ctx, flags: u8, samples: u32, wave: &mut [u8]) -> bool {
        let width = 1u32 << self.info.subband;
        let count = (samples + width - 1) & !(width - 1);
        let subbands = (count >> self.info.subband) as usize;
        let all = count as usize * self.info.channels;
        if ctx.bits.bit() != 0 {
            return false;
        }
        self.division.clear();
        self.revcode.clear();
        self.weight.clear();
        self.coef.clear();
        let mut last = -1;
        for i in 0..subbands {
            let code = ctx.bits.bits(2) as i32;
            self.division.push(code as u8);
            let mut lead = false;
            if code != last {
                if i != 0 {
                    self.revcode.push(ctx.bits.bits(2) as u8);
                    self.weight.push(ctx.bits.bits(32) as i32);
                    self.coef.push(ctx.bits.bits(16) as i32);
                }
                lead = true;
                last = code;
            }
            for _ in 0..1 << code {
                let r = if lead {
                    lead = false;
                    ctx.bits.bits(2)
                } else {
                    ctx.bits.bits(4)
                };
                self.revcode.push(r as u8);
                self.weight.push(ctx.bits.bits(32) as i32);
                self.coef.push(ctx.bits.bits(16) as i32);
            }
        }
        if subbands > 0 {
            self.revcode.push(ctx.bits.bits(2) as u8);
            self.weight.push(ctx.bits.bits(32) as i32);
            self.coef.push(ctx.bits.bits(16) as i32);
        }
        if ctx.bits.bit() != 0 {
            return false;
        }
        if flags & LEAD_BLOCK != 0 {
            self.prepare(ctx);
        }
        if !self.read_coefficients(ctx, all, width as usize * 2, subbands) {
            return false;
        }
        let mut rest = samples;
        let mut at = 0;
        let mut last = -1;
        self.next_rev = 0;
        self.next_weight = 0;
        self.next_coef = 0;
        self.next_src = 0;
        let sub = self.info.subband;
        for i in 0..subbands {
            let code = self.division[i] as i32;
            let mut lead = false;
            if last != code {
                if i != 0 {
                    let n = rest.min(self.num as u32);
                    self.post_block_mss(wave, at, n);
                    rest -= n;
                    at += n as usize * 4;
                }
                self.init_degree(sub - code);
                last = code;
                lead = true;
            }
            for _ in 0..1 << code {
                if lead {
                    self.lead_block_mss();
                    lead = false;
                } else {
                    let n = rest.min(self.num as u32);
                    self.internal_block_mss(wave, at, n);
                    rest -= n;
                    at += n as usize * 4;
                }
            }
        }
        if subbands > 0 {
            let n = rest.min(self.num as u32);
            self.post_block_mss(wave, at, n);
        }
        true
    }

    fn rev(&mut self) -> i32 {
        let v = self.revcode.get(self.next_rev).copied().unwrap_or(0) as i32;
        self.next_rev += 1;
        v
    }

    fn lead_block_mss(&mut self) {
        let num = self.num;
        let (w, c) = self.next_codes();
        let mut l = std::mem::take(&mut self.last_dct);
        for i in 0..2 {
            self.half_source();
            let src = self.buf1[..num].to_vec();
            self.iquantumize(&mut l[i * num..i * num + num], &src, w, c);
        }
        let r = self.rev();
        let (s, co) = sin_cos(r);
        revolve2x2(&mut l, 0, num, s, co, 1, num);
        for i in 0..2 {
            let o = i * num;
            odd_givens_inverse(&mut l[o..o + num], &self.revolve, self.degree);
            for j in (0..num).step_by(2) {
                l[o + j] = l[o + j + 1];
            }
            fast_iplot(&mut l[o..o + num]);
        }
        self.last_dct = l;
    }

    fn internal_block_mss(&mut self, wave: &mut [u8], at: usize, n: u32) {
        let num = self.num;
        let (w, c) = self.next_codes();
        let mut m = std::mem::take(&mut self.matrix);
        for i in 0..2 {
            let src: Vec<i32> = (0..num).map(|_| self.src()).collect();
            self.iquantumize(&mut m[i * num..i * num + num], &src, w, c);
        }
        let r = self.rev();
        let (s1, c1) = sin_cos((r >> 2) & 3);
        revolve2x2(&mut m, 0, num, s1, c1, 2, num / 2);
        let (s2, c2) = sin_cos(r & 3);
        revolve2x2(&mut m, 1, num + 1, s2, c2, 2, num / 2);
        for i in 0..2 {
            self.decode_tail(&mut m, i * num, i * num, true);
            if n != 0 {
                round_words(wave, at + i * 2, 2, &self.internal, n as usize);
            }
        }
        self.matrix = m;
    }

    fn post_block_mss(&mut self, wave: &mut [u8], at: usize, n: u32) {
        let num = self.num;
        let (w, c) = self.next_codes();
        let mut m = std::mem::take(&mut self.matrix);
        for i in 0..2 {
            self.half_source();
            let src = self.buf1[..num].to_vec();
            self.iquantumize(&mut m[i * num..i * num + num], &src, w, c);
        }
        let r = self.rev();
        let (s, co) = sin_cos(r);
        revolve2x2(&mut m, 0, num, s, co, 1, num);
        for i in 0..2 {
            let o = i * num;
            odd_givens_inverse(&mut m[o..o + num], &self.revolve, self.degree);
            for j in (0..num).step_by(2) {
                m[o + j] = -m[o + j + 1];
            }
            self.decode_tail(&mut m, o, o, false);
            if n != 0 {
                round_words(wave, at + i * 2, 2, &self.internal, n as usize);
            }
        }
        self.matrix = m;
    }

    fn iquantumize(&mut self, dst: &mut [f32], q: &[i32], weight: i32, coef: i32) {
        let num = dst.len();
        let scale = (2.0 / num as f64).sqrt();
        let rc = scale * coef as f64;
        let mut avg = [1.0f64; 7];
        for (i, a) in avg.iter_mut().take(6).enumerate() {
            *a = 1.0 / 2f64.powf((((weight >> (i * 5)) & 0x1f) - 15) as f64 * 0.5);
        }
        let t = &mut self.weight_table;
        let f = &self.freq;
        let mut i = 0usize;
        while (i as i32) < f[0] && i < num {
            t[i] = avg[0] as f32;
            i += 1;
        }
        for j in 1..7 {
            let a = avg[j - 1];
            let k = (avg[j] - a) / (f[j] - f[j - 1]) as f64;
            while (i as i32) < f[j] && i < num {
                t[i] = (k * (i as i32 - f[j - 1]) as f64 + a) as f32;
                i += 1;
            }
        }
        while i < num {
            t[i] = avg[6] as f32;
            i += 1;
        }
        let odd = ((((weight >> 30) & 3) + 2) as f64 / 2.0) as f32;
        let mut i = 15;
        while i < num {
            t[i] *= odd;
            i += 16;
        }
        t[num - 1] = coef as f32;
        for v in t[..num].iter_mut() {
            *v = 1.0 / *v;
        }
        for i in 0..num {
            dst[i] = (rc * t[i] as f64 * q[i] as f64) as f32;
        }
    }
}

fn sin_cos(code: i32) -> (f32, f32) {
    let a = code as f64 * std::f64::consts::PI / 8.0;
    (a.sin() as f32, a.cos() as f32)
}

fn round_words(dst: &mut [u8], mut at: usize, step: usize, src: &[f32], n: usize) {
    for &r in &src[..n.min(src.len())] {
        let v = if r >= 0.0 {
            (r + 0.5).floor()
        } else {
            (r - 0.5).ceil()
        };
        let v = v.clamp(-32768.0, 32767.0) as i16;
        if at + 2 <= dst.len() {
            dst[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
        at += step * 2;
    }
}

fn revolve_parameter(degree: i32) -> Vec<(f32, f32)> {
    let num = 1i32 << degree;
    let mut lc = 1;
    let mut n = num / 2;
    while n >= 8 {
        lc += 1;
        n /= 8;
    }
    let mut r = vec![(0f32, 0f32); lc * 8];
    let k = std::f64::consts::PI / (num as f64 * 2.0);
    let mut next = 0;
    let mut step = 2;
    loop {
        for i in 0..7 {
            let mut ws = 1.0f64;
            let mut a = 0.0f64;
            for j in 0..i {
                a += step as f64;
                ws = ws * r[next + j].0 as f64 + r[next + j].1 as f64 * (a * k).cos();
            }
            let x = ws.atan2(((a + step as f64) * k).cos());
            r[next + i] = (x.sin() as f32, x.cos() as f32);
        }
        next += 7;
        step *= 8;
        if step >= num {
            break;
        }
    }
    r
}

fn odd_givens_inverse(s: &mut [f32], rev: &[(f32, f32)], degree: i32) {
    let num = 1usize << degree;
    let mut index = 1usize;
    let mut step = 2usize;
    let mut lc = (num / 2) / 8;
    let mut ri = 0usize;
    loop {
        ri += 7;
        index += step * 7;
        step *= 8;
        if lc <= 8 {
            break;
        }
        lc /= 8;
    }
    let rot = |s: &mut [f32], k: usize, step: usize, (sn, cs): (f32, f32)| {
        let r1 = s[k];
        let r2 = s[k + step];
        s[k] = r1 * cs + r2 * sn;
        s[k + step] = r2 * cs - r1 * sn;
    };
    if lc >= 2 {
        let mut k = index + step * (lc - 2);
        for j in (0..=lc - 2).rev() {
            rot(s, k, step, rev[ri + j]);
            k = k.wrapping_sub(step);
        }
    }
    while lc <= (num / 2) / 8 {
        ri -= 7;
        step /= 8;
        index -= step * 7;
        for i in 0..lc {
            let mut k = i * (step * 8) + index + step * 6;
            for j in (0..=6).rev() {
                rot(s, k, step, rev[ri + j]);
                k = k.wrapping_sub(step);
            }
        }
        lc *= 8;
    }
}

fn fast_iplot(s: &mut [f32]) {
    for i in (0..s.len()).step_by(2) {
        let (r1, r2) = (s[i], s[i + 1]);
        s[i] = 0.5 * (r1 + r2);
        s[i + 1] = 0.5 * (r1 - r2);
    }
}

fn revolve2x2(
    b: &mut [f32],
    mut p1: usize,
    mut p2: usize,
    sn: f32,
    cs: f32,
    step: usize,
    n: usize,
) {
    for _ in 0..n {
        let (r1, r2) = (b[p1], b[p2]);
        b[p1] = r1 * cs - r2 * sn;
        b[p2] = r1 * sn + r2 * cs;
        p1 += step;
        p2 += step;
    }
}

const COS_PI4: f32 = std::f32::consts::FRAC_1_SQRT_2;
const TWO_COS_PI4: f32 = 2.0 * COS_PI4;

/// DCT of `src` (destroyed) into `dst[d0 + i * dstep]`.
fn fast_dct(k: &[Vec<f32>], dst: &mut [f32], d0: usize, dstep: usize, src: &mut [f32], deg: i32) {
    if deg == 2 {
        let mut r = [
            src[0] + src[3],
            src[1] + src[2],
            src[0] - src[3],
            src[1] - src[2],
        ];
        dst[d0] = 0.5 * (r[0] + r[1]);
        dst[d0 + dstep * 2] = COS_PI4 * (r[0] - r[1]);
        r[2] *= k[1][0];
        r[3] *= k[1][1];
        r[0] = r[2] + r[3];
        r[1] = TWO_COS_PI4 * (r[2] - r[3]);
        r[1] -= r[0];
        dst[d0 + dstep] = r[0];
        dst[d0 + dstep * 3] = r[1];
        return;
    }
    let n = 1usize << deg;
    let half = n / 2;
    let mut w = vec![0f32; n];
    for i in 0..half {
        w[i] = src[i] + src[n - i - 1];
        w[i + half] = src[i] - src[n - i - 1];
    }
    let (a, b) = w.split_at_mut(half);
    let step = dstep * 2;
    fast_dct(k, dst, d0, step, a, deg - 1);
    let kk = &k[deg as usize - 1];
    for i in 0..half {
        b[i] *= kk[i];
    }
    let d1 = d0 + dstep;
    fast_dct(k, dst, d1, step, b, deg - 1);
    let mut p = d1;
    for _ in 0..half {
        dst[p] += dst[p];
        p += step;
    }
    let mut p = d1;
    for _ in 1..half {
        dst[p + step] -= dst[p];
        p += step;
    }
}

/// Inverse DCT of `src[s0 + i * sstep]` into `dst[..n]`.
fn fast_idct(k: &[Vec<f32>], dst: &mut [f32], src: &[f32], s0: usize, sstep: usize, deg: i32) {
    if deg == 2 {
        let a0 = src[s0];
        let a1 = COS_PI4 * src[s0 + sstep * 2];
        let b0 = a0 + a1;
        let b1 = a0 - a1;
        let c0 = k[1][0] * src[s0 + sstep];
        let c1 = k[1][1] * src[s0 + sstep * 3];
        let b2 = c0 + c1;
        let mut b3 = TWO_COS_PI4 * (c0 - c1);
        b3 -= b2;
        dst[0] = b0 + b2;
        dst[3] = b0 - b2;
        dst[1] = b1 + b3;
        dst[2] = b1 - b3;
        return;
    }
    let n = 1usize << deg;
    let half = n / 2;
    let step = sstep * 2;
    fast_idct(k, dst, src, s0, step, deg - 1);
    let kk = &k[deg as usize - 1];
    let mut w = vec![0f32; half];
    let mut p = s0 + sstep;
    for i in 0..half {
        w[i] = src[p] * kk[i];
        p += step;
    }
    fast_dct(k, dst, half, 1, &mut w, deg - 1);
    for i in 0..half {
        dst[half + i] += dst[half + i];
    }
    for i in 1..half {
        dst[half + i] -= dst[half + i - 1];
    }
    for i in 0..half / 2 {
        let r0 = dst[i] + dst[half + i];
        let r3 = dst[i] - dst[half + i];
        let r1 = dst[half - 1 - i] + dst[n - 1 - i];
        let r2 = dst[half - 1 - i] - dst[n - 1 - i];
        dst[i] = r0;
        dst[half - 1 - i] = r1;
        dst[half + i] = r2;
        dst[n - 1 - i] = r3;
    }
}
