//! The ERISA coders of Entis: the big-endian bit reader, the adaptive arithmetic code
//! ("ERISA code"), the ERISA-N (Nemesis) byte compressor that packs archive entries and the
//! exe's archive key list, and the BSHF bit-shuffle cipher of encrypted archive entries.

/// Reads bits from the top of big-endian 32-bit words (the input is padded with zeros to 4).
pub struct Bits<'a> {
    src: &'a [u8],
    pos: usize,
    word: u32,
    left: u32,
}

impl<'a> Bits<'a> {
    pub fn new(src: &'a [u8]) -> Bits<'a> {
        Bits {
            src,
            pos: 0,
            word: 0,
            left: 0,
        }
    }

    fn fill(&mut self) -> bool {
        if self.left == 0 {
            if self.pos >= self.src.len() {
                return false;
            }
            let mut b = [0u8; 4];
            for (i, v) in b.iter_mut().enumerate() {
                *v = self.src.get(self.pos + i).copied().unwrap_or(0);
            }
            self.pos += 4;
            self.word = u32::from_be_bytes(b);
            self.left = 32;
        }
        true
    }

    /// One bit as 0 or -1; past the end, 1.
    pub fn bit(&mut self) -> i32 {
        if !self.fill() {
            return 1;
        }
        let v = (self.word as i32) >> 31;
        self.left -= 1;
        self.word <<= 1;
        v
    }

    pub fn bits(&mut self, mut n: u32) -> u32 {
        let mut code = 0u32;
        while n != 0 {
            if !self.fill() {
                break;
            }
            let k = n.min(self.left);
            code = ((code as u64) << k) as u32 | (self.word >> (32 - k));
            n -= k;
            self.left -= k;
            self.word = ((self.word as u64) << k) as u32;
        }
        code
    }

    /// Drops the rest of the current word.
    pub fn flush(&mut self) {
        self.left = 0;
    }

    /// The bytes read so far (whole words).
    pub fn consumed(&self) -> usize {
        self.pos
    }
}

pub const ESC: i16 = -1;
const TOTAL_LIMIT: u32 = 0x2000;
const SYMBOL_SORTS: usize = 0x101;
const SUB_SORTS: usize = 0x80;

/// An adaptive symbol frequency table, kept sorted by frequency.
#[derive(Clone)]
pub struct Model {
    total: u32,
    sorts: usize,
    /// (occurrences, symbol)
    sym: [(u16, i16); SYMBOL_SORTS],
    sub: [(u16, i16); SUB_SORTS],
}

impl Model {
    pub fn new() -> Model {
        let mut m = Model {
            total: SYMBOL_SORTS as u32,
            sorts: SYMBOL_SORTS,
            sym: [(1, 0); SYMBOL_SORTS],
            sub: [(0, -1); SUB_SORTS],
        };
        for i in 0..0x100 {
            m.sym[i] = (1, i as i16);
        }
        m.sym[0x100] = (1, ESC);
        m
    }

    fn empty() -> Model {
        Model {
            total: 0,
            sorts: 0,
            sym: [(0, 0); SYMBOL_SORTS],
            sub: [(0, -1); SUB_SORTS],
        }
    }

    fn half(&mut self) {
        self.total = 0;
        for i in 0..self.sorts {
            self.sym[i].0 = self.sym[i].0.div_ceil(2);
            self.total += self.sym[i].0 as u32;
        }
        for s in &mut self.sub {
            s.0 >>= 1;
        }
    }

    fn increase(&mut self, mut index: usize) -> usize {
        self.sym[index].0 += 1;
        let (occ, symbol) = self.sym[index];
        while index > 0 && self.sym[index - 1].0 < occ {
            self.sym[index] = self.sym[index - 1];
            index -= 1;
        }
        self.sym[index] = (occ, symbol);
        self.total += 1;
        if self.total >= TOTAL_LIMIT {
            self.half();
        }
        index
    }

    fn add(&mut self, symbol: i16) {
        let i = self.sorts;
        self.sorts += 1;
        self.total += 1;
        self.sym[i] = (1, symbol);
    }
}

impl Default for Model {
    fn default() -> Self {
        Model::new()
    }
}

/// The arithmetic decoder.
pub struct Arith<'a> {
    pub bits: Bits<'a>,
    code: u32,
    augend: u32,
    post: u32,
}

impl<'a> Arith<'a> {
    pub fn new(src: &'a [u8]) -> Arith<'a> {
        Arith::from_bits(Bits::new(src))
    }

    /// Starts the code at the current position of a bit reader.
    pub fn from_bits(mut bits: Bits<'a>) -> Arith<'a> {
        let code = bits.bits(32);
        Arith {
            bits,
            code,
            augend: 0xffff,
            post: 0,
        }
    }

    /// The index in the model's table of the next symbol, or None at the end of the code.
    pub fn index(&mut self, m: &Model) -> Option<usize> {
        let acc = (self.code as u64 * m.total as u64 / self.augend as u64) as u32;
        if acc >= TOTAL_LIMIT {
            return None;
        }
        let mut acc = acc as u16;
        let mut fs: u32 = 0;
        let mut i = 0;
        let occ;
        loop {
            let o = m.sym[i].0;
            if acc < o {
                occ = o;
                break;
            }
            acc -= o;
            fs += o as u32;
            i += 1;
            if i >= m.sorts {
                return None;
            }
        }
        let total = m.total as u64;
        self.code = self
            .code
            .wrapping_sub((self.augend as u64 * fs as u64).div_ceil(total) as u32);
        self.augend = (self.augend as u64 * occ as u64 / total) as u32;
        if self.augend == 0 {
            return None;
        }
        while self.augend & 0x8000 == 0 {
            let mut b = self.bits.bit();
            if b == 1 {
                self.post += 1;
                if self.post >= 256 {
                    return None;
                }
                b = 0;
            }
            self.code = (self.code << 1) | (b as u32 & 1);
            self.augend <<= 1;
        }
        self.code &= 0xffff;
        Some(i)
    }

    /// The next symbol (ESC at the end), counted in the model.
    pub fn symbol(&mut self, m: &mut Model) -> i16 {
        match self.index(m) {
            Some(i) => {
                let s = m.sym[i].1;
                m.increase(i);
                s
            }
            None => ESC,
        }
    }
}

/// ERISA-coded bytes (lossless ERI pictures of the Nemesis architecture): each byte is coded in
/// the model of the byte before it; a zero is followed by the count of zeros after it.
pub struct ErisaBytes {
    table: Vec<Model>,
    run: Model,
    last: usize,
    length: u32,
}

impl Default for ErisaBytes {
    fn default() -> Self {
        ErisaBytes {
            table: vec![Model::new(); 0x101],
            run: Model::new(),
            last: 0,
            length: 0,
        }
    }
}

impl ErisaBytes {
    pub fn decode(&mut self, a: &mut Arith, dst: &mut [i8]) -> usize {
        let mut i = 0;
        let mut p = self.last;
        while i < dst.len() {
            if self.length > 0 {
                let n = (dst.len() - i).min(self.length as usize);
                self.length -= n as u32;
                dst[i..i + n].iter_mut().for_each(|v| *v = 0);
                i += n;
                continue;
            }
            let Some(k) = a.index(&self.table[p]) else {
                break;
            };
            let sym = self.table[p].sym[k].1;
            self.table[p].increase(k);
            dst[i] = sym as i8;
            i += 1;
            if sym == 0 {
                let Some(k) = a.index(&self.run) else {
                    break;
                };
                self.length = self.run.sym[k].1 as u16 as u32;
                self.run.increase(k);
            }
            p = (sym as i32 & 0xff) as usize;
        }
        self.last = p;
        i
    }
}

const SHIFT: [u32; 4] = [1, 3, 4, 5];
const NEW_PROB_LIMIT: [u16; 4] = [1, 8, 16, 32];
const SLOT_MAX: usize = 0x800;
const NBUF: usize = 0x10000;

/// Decodes ERISA-N (Nemesis) data; stops after `limit` bytes or at the end of the code.
pub fn nemesis(src: &[u8], limit: usize) -> Vec<u8> {
    let mut a = Arith::new(src);
    let mut out = Vec::new();
    // models[0] is the base model; models[1 + i] the context slots
    let mut models: Vec<Model> = vec![Model::new()];
    let mut phrase_len = Model::new();
    let mut phrase_index = Model::new();
    let mut run_len = Model::new();
    let mut last = [0u8; 4];
    let mut ilast = 0usize;
    let mut buf = vec![0u8; NBUF];
    let mut bi = 0usize;
    // per byte value: ring of the positions after which it was written
    let mut lookup: Vec<(usize, [u32; 0x100])> = vec![(0, [0; 0x100]); 0x100];
    let push = |s: u8,
                out: &mut Vec<u8>,
                last: &mut [u8; 4],
                ilast: &mut usize,
                buf: &mut Vec<u8>,
                bi: &mut usize,
                lookup: &mut Vec<(usize, [u32; 0x100])>| {
        last[*ilast] = s;
        *ilast = (*ilast + 1) & 3;
        let l = &mut lookup[s as usize];
        l.1[l.0] = *bi as u32;
        l.0 = (l.0 + 1) & 0xff;
        buf[*bi] = s;
        *bi = (*bi + 1) & (NBUF - 1);
        out.push(s);
    };
    while out.len() < limit {
        let mut deg = 0;
        let mut mi = 0usize;
        while deg < 4 {
            let il = (last[(ilast + 3 - deg) & 3] >> SHIFT[deg]) as usize;
            let s = models[mi].sub[il].1;
            if s < 0 {
                break;
            }
            if s as usize + 1 >= models.len() {
                return out;
            }
            mi = s as usize + 1;
            deg += 1;
        }
        let Some(isym) = a.index(&models[mi]) else {
            return out;
        };
        let mut symbol = models[mi].sym[isym].1;
        models[mi].increase(isym);
        let mut nemesis = false;
        if symbol == ESC {
            if mi != 0 {
                let Some(i0) = a.index(&models[0]) else {
                    return out;
                };
                symbol = models[0].sym[i0].1;
                models[0].increase(i0);
                if symbol != ESC {
                    models[mi].add(symbol);
                } else {
                    nemesis = true;
                }
            } else {
                nemesis = true;
            }
        }
        if nemesis {
            let pi = a.symbol(&mut phrase_index);
            if pi == ESC {
                return out;
            }
            let len = if pi == 0 {
                a.symbol(&mut run_len)
            } else {
                a.symbol(&mut phrase_len)
            };
            if len == ESC {
                return out;
            }
            let lastsym = buf[(bi + NBUF - 1) & (NBUF - 1)];
            let mut next: Option<usize> = None;
            if pi != 0 {
                let l = &lookup[lastsym as usize];
                let p = l.1[(l.0.wrapping_sub(pi as usize)) & 0xff] as usize;
                if buf[p] != lastsym {
                    return out;
                }
                next = Some((p + 1) & (NBUF - 1));
            }
            let mut prev = lastsym;
            for _ in 0..len {
                if out.len() >= limit {
                    break;
                }
                let s = match &mut next {
                    Some(n) => {
                        let s = buf[*n];
                        *n = (*n + 1) & (NBUF - 1);
                        s
                    }
                    None => prev,
                };
                push(
                    s,
                    &mut out,
                    &mut last,
                    &mut ilast,
                    &mut buf,
                    &mut bi,
                    &mut lookup,
                );
                prev = s;
            }
            continue;
        }
        let s = symbol as u8;
        push(
            s,
            &mut out,
            &mut last,
            &mut ilast,
            &mut buf,
            &mut bi,
            &mut lookup,
        );
        if models.len() - 1 < SLOT_MAX && deg < 4 {
            let isub = (s >> SHIFT[deg]) as usize;
            models[mi].sub[isub].0 += 1;
            if models[mi].sub[isub].0 >= NEW_PROB_LIMIT[deg] {
                let parent = mi;
                let mut m = 0usize;
                let mut i = 0;
                let mut is = 0usize;
                while i <= deg {
                    is = (last[(ilast + 3 - i) & 3] >> SHIFT[i]) as usize;
                    let sy = models[m].sub[is].1;
                    if sy < 0 {
                        break;
                    }
                    if sy as usize + 1 >= models.len() {
                        return out;
                    }
                    m = sy as usize + 1;
                    i += 1;
                }
                if i <= deg && models[m].sub[is].1 < 0 {
                    let slot = models.len() - 1;
                    models[m].sub[is].1 = slot as i16;
                    let mut new = Model::empty();
                    let p = &models[parent];
                    for k in 0..p.sorts {
                        let o = p.sym[k].0 >> 4;
                        if o > 0 && p.sym[k].1 != ESC {
                            new.total += o as u32;
                            new.sym[new.sorts] = (o, p.sym[k].1);
                            new.sorts += 1;
                        }
                    }
                    new.total += 1;
                    new.sym[new.sorts] = (1, ESC);
                    new.sorts += 1;
                    models.push(new);
                }
            }
        }
    }
    out
}

/// Decrypts BSHF data: each 32 bytes are a bit permutation keyed by the password.
pub fn bshf(src: &[u8], password: &str) -> Vec<u8> {
    let mut pass: Vec<u8> = password.bytes().collect();
    if pass.is_empty() {
        pass.push(b' ');
    }
    if pass.len() < 32 {
        pass.push(0x1b);
        let n = pass.len();
        for i in n..32 {
            let v = pass[i % n].wrapping_add(pass[i - 1]);
            pass.push(v);
        }
    }
    let plen = pass.len();
    let mut out = Vec::with_capacity(src.len());
    let mut poff = 0usize;
    for block in src.chunks_exact(32) {
        if poff >= plen {
            poff = 0;
        }
        let mut dst = [0u8; 32];
        let mut mask = [0u8; 32];
        let mut ipos = poff;
        poff += 1;
        let mut ibit: usize = 0;
        for i in 0..256 {
            ibit = (ibit + pass[ipos] as usize) & 0xff;
            ipos += 1;
            if ipos >= plen {
                ipos = 0;
            }
            let mut off = ibit >> 3;
            let mut m: u32 = 0x80 >> (ibit & 7);
            while mask[off] == 0xff {
                ibit = (ibit + 8) & 0xff;
                off = ibit >> 3;
            }
            while mask[off] as u32 & m != 0 {
                ibit += 1;
                m >>= 1;
                if m == 0 {
                    ibit = (ibit + 8) & 0xff;
                    off = ibit >> 3;
                    m = 0x80;
                }
            }
            mask[off] |= m as u8;
            if block[i >> 3] & (0x80 >> (i & 7)) != 0 {
                dst[off] |= m as u8;
            }
        }
        out.extend_from_slice(&dst);
    }
    out
}
