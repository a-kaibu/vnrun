//! ERI pictures ("Entis Rasterized Image"), the lossless kind (gamma run-length and adaptive
//! Huffman "ERINA" codes) that Entis GLS games use. Ported from GARbro (MIT, morkt).

use crate::erisa::Bits;

pub struct Eri {
    pub w: u32,
    pub h: u32,
    /// RGBA, top row first; 8-bit pictures are gray with the gray as alpha too
    pub px: Vec<u8>,
    /// the hot spot from the description ("#hot-spot" x,y), or 0,0
    pub hot: (i32, i32),
    /// 8 bits a pixel (a mask or a gray picture)
    pub gray: bool,
}

struct Info {
    version: u32,
    transform: u32,
    arch: i32,
    format: u32,
    w: u32,
    h: u32,
    flip: bool,
    bpp: u32,
    blocking: u32,
}

fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u64le(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

pub fn decode(b: &[u8]) -> Result<Eri, String> {
    let bad = || "eri: bad header".to_string();
    if !(b.starts_with(b"Entis\x1a") && b.get(0x40..0x48) == Some(b"Header  ")) {
        return Err("eri: not an ERI picture".into());
    }
    let hlen = u64le(b, 0x48).ok_or_else(bad)? as usize;
    let mut p = 0x50;
    let end = 0x50 + hlen;
    let mut info = None;
    let mut hot = (0, 0);
    while p + 16 <= end {
        let id = &b[p..p + 8];
        let len = u64le(b, p + 8).ok_or_else(bad)? as usize;
        let d = b.get(p + 16..p + 16 + len).ok_or_else(bad)?;
        if id == b"ImageInf" {
            let h = u32le(d, 20).ok_or_else(bad)? as i32;
            info = Some(Info {
                version: u32le(d, 0).ok_or_else(bad)?,
                transform: u32le(d, 4).ok_or_else(bad)?,
                arch: u32le(d, 8).ok_or_else(bad)? as i32,
                format: u32le(d, 12).ok_or_else(bad)?,
                w: (u32le(d, 16).ok_or_else(bad)? as i32).unsigned_abs(),
                h: h.unsigned_abs(),
                flip: h < 0,
                bpp: u32le(d, 24).ok_or_else(bad)?,
                blocking: u32le(d, 52).ok_or_else(bad)?,
            });
        } else if id == b"descript" {
            hot = hot_spot(d).unwrap_or((0, 0));
        }
        p += 16 + len;
    }
    let info = info.ok_or_else(bad)?;
    // the stream: find "ImageFrm"
    p = end;
    let mut frame = None;
    while p + 16 <= b.len() {
        let id = &b[p..p + 8];
        let len = u64le(b, p + 8).ok_or_else(bad)? as usize;
        if id == b"Stream  " {
            p += 16;
            continue;
        }
        if id == b"ImageFrm" {
            frame = Some(&b[p + 16..(p + 16 + len).min(b.len())]);
            break;
        }
        p += 16 + len;
    }
    let frame = frame.ok_or("eri: no image frame")?;
    if info.transform != 0x0302_0000 {
        return Err(format!(
            "eri: transformation {:#x} not supported",
            info.transform
        ));
    }
    let px = Lossless::new(&info)?.decode(frame)?;
    Ok(Eri {
        w: info.w,
        h: info.h,
        px,
        hot,
        gray: info.bpp == 8,
    })
}

fn hot_spot(d: &[u8]) -> Option<(i32, i32)> {
    let text = if d.starts_with(&[0xff, 0xfe]) {
        let u: Vec<u16> = d[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(d).into_owned()
    };
    let mut lines = text.lines();
    while let Some(l) = lines.next() {
        if l.trim().trim_start_matches('#').trim() == "hot-spot" {
            let v = lines.next()?;
            let mut it = v.split(',').map(|s| s.trim().parse::<i32>());
            return Some((it.next()?.ok()?, it.next()?.ok()?));
        }
    }
    None
}

const CODE_FLAG: i32 = i32::MIN;
const ESCAPE: i32 = 0x7fff_ffff;
const NULL: i32 = 0x8000;
const MAX: u16 = 0x4000;
const ROOT: usize = 0x200;

#[derive(Clone, Copy, Default)]
struct Node {
    weight: u16,
    parent: u16,
    child: i32,
}

#[derive(Clone)]
struct Tree {
    n: Vec<Node>,
    lookup: [i32; 0x100],
    escape: i32,
    ptr: usize,
}

impl Tree {
    fn new() -> Tree {
        let mut t = Tree {
            n: vec![Node::default(); 0x201],
            lookup: [NULL; 0x100],
            escape: NULL,
            ptr: ROOT,
        };
        t.n[ROOT] = Node {
            weight: 0,
            parent: NULL as u16,
            child: NULL,
        };
        t
    }

    fn increase(&mut self, e: usize) {
        self.n[e].weight += 1;
        self.normalize(e);
        if self.n[ROOT].weight >= MAX {
            self.half_and_rebuild();
        }
    }

    fn recount(&mut self, p: usize) {
        let c = self.n[p].child as usize;
        self.n[p].weight = self.n[c].weight.wrapping_add(self.n[c + 1].weight);
    }

    fn relink(&mut self, at: usize) {
        let c = self.n[at].child;
        if c & CODE_FLAG == 0 {
            self.n[c as usize].parent = at as u16;
            self.n[c as usize + 1].parent = at as u16;
        } else {
            let code = c & !CODE_FLAG;
            if code != ESCAPE {
                self.lookup[(code & 0xff) as usize] = at as i32;
            } else {
                self.escape = at as i32;
            }
        }
    }

    fn normalize(&mut self, mut e: usize) {
        while e < ROOT {
            let mut s = e + 1;
            let w = self.n[e].weight;
            while s < ROOT && self.n[s].weight < w {
                s += 1;
            }
            s -= 1;
            if s == e {
                e = self.n[e].parent as usize;
                self.recount(e);
                continue;
            }
            let ep = self.n[e].parent;
            let sp = self.n[s].parent;
            self.n.swap(e, s);
            self.n[s].parent = sp;
            self.n[e].parent = ep;
            self.relink(s);
            self.relink(e);
            self.recount(sp as usize);
            e = sp as usize;
        }
    }

    fn add(&mut self, code: i32) {
        if self.ptr > 0 {
            self.ptr -= 2;
            let i = self.ptr;
            self.n[i].weight = 1;
            self.n[i].child = CODE_FLAG | code;
            self.lookup[(code & 0xff) as usize] = i as i32;
            if self.n[ROOT].child != NULL {
                self.n[i + 1] = self.n[i + 2];
                let c = self.n[i + 1].child;
                if c & CODE_FLAG != 0 {
                    let cc = c & !CODE_FLAG;
                    if cc != ESCAPE {
                        self.lookup[(cc & 0xff) as usize] = i as i32 + 1;
                    } else {
                        self.escape = i as i32 + 1;
                    }
                }
                let parent = self.n[i + 1].parent;
                self.n[i + 2].weight = self.n[i].weight + self.n[i + 1].weight;
                self.n[i + 2].parent = parent;
                self.n[i + 2].child = i as i32;
                self.n[i].parent = (i + 2) as u16;
                self.n[i + 1].parent = (i + 2) as u16;
                self.normalize(i + 2);
            } else {
                self.n[i].parent = ROOT as u16;
                self.escape = i as i32 + 1;
                self.n[i + 1] = Node {
                    weight: 1,
                    parent: ROOT as u16,
                    child: CODE_FLAG | ESCAPE,
                };
                self.n[ROOT].weight = 2;
                self.n[ROOT].child = i as i32;
            }
        } else {
            let mut i = self.ptr;
            if self.n[i].child == (CODE_FLAG | ESCAPE) {
                i += 1;
            }
            self.n[i].child = CODE_FLAG | code;
        }
    }

    fn half_and_rebuild(&mut self) {
        let mut next = ROOT;
        let mut i = ROOT - 1;
        loop {
            if i < self.ptr {
                break;
            }
            if self.n[i].child & CODE_FLAG != 0 {
                self.n[i].weight = self.n[i].weight.div_ceil(2);
                self.n[next] = self.n[i];
                next -= 1;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        next += 1;
        let mut i = self.ptr;
        loop {
            self.n[i] = self.n[next];
            self.n[i + 1] = self.n[next + 1];
            next += 2;
            self.relink(i);
            self.relink(i + 1);
            let w = self.n[i].weight.wrapping_add(self.n[i + 1].weight);
            if next <= ROOT {
                let mut j = next;
                loop {
                    if w <= self.n[j].weight {
                        self.n[j - 1].weight = w;
                        self.n[j - 1].child = i as i32;
                        break;
                    }
                    self.n[j - 1] = self.n[j];
                    j += 1;
                    if j > ROOT {
                        self.n[ROOT].weight = w;
                        self.n[ROOT].child = i as i32;
                        break;
                    }
                }
                next -= 1;
            } else {
                self.n[ROOT].weight = w;
                self.n[ROOT].parent = NULL as u16;
                self.n[ROOT].child = i as i32;
                self.n[i].parent = ROOT as u16;
                self.n[i + 1].parent = ROOT as u16;
                break;
            }
            i += 2;
        }
    }
}

/// The bit-level decoders of lossless ERI.
struct Ctx<'a> {
    bits: Bits<'a>,
    zero: i32,
    length: u32,
    trees: Vec<Tree>,
    last_tree: usize,
}

impl<'a> Ctx<'a> {
    fn gamma(&mut self) -> i32 {
        gamma(&mut self.bits)
    }

    fn init_gamma(&mut self) {
        self.zero = self.bits.bit();
        self.length = 0;
    }

    fn gamma_bytes(&mut self, dst: &mut [i8]) -> usize {
        let mut count = dst.len() as u32;
        let mut d = 0usize;
        let mut decoded = 0usize;
        if self.length == 0 {
            self.length = self.gamma() as u32;
            if self.length == 0 {
                return 0;
            }
        }
        loop {
            let mut rep = self.length.min(count);
            self.length -= rep;
            count -= rep;
            if self.zero == 0 {
                decoded += rep as usize;
                while rep > 0 {
                    dst[d] = 0;
                    d += 1;
                    rep -= 1;
                }
            } else {
                while rep > 0 {
                    let sign = self.bits.bit() as i8;
                    let code = self.gamma() as i8;
                    if code == 0 {
                        return decoded;
                    }
                    decoded += 1;
                    dst[d] = (code ^ sign).wrapping_sub(sign);
                    d += 1;
                    rep -= 1;
                }
            }
            if count == 0 {
                if self.length == 0 {
                    self.zero = !self.zero;
                }
                return decoded;
            }
            self.zero = !self.zero;
            self.length = self.gamma() as u32;
            if self.length == 0 {
                return decoded;
            }
        }
    }

    fn prepare_erina(&mut self) {
        self.trees = vec![Tree::new(); 0x101];
        self.length = 0;
        self.last_tree = 0;
    }

    fn erina_bytes(&mut self, dst: &mut [i8]) -> usize {
        let n = dst.len();
        let mut t = self.last_tree;
        let mut i = 0;
        if self.length > 0 {
            let l = (self.length as usize).min(n);
            self.length -= l as u32;
            for v in &mut dst[..l] {
                *v = 0;
            }
            i = l;
        }
        while i < n {
            let s = huffman(&mut self.bits, &mut self.trees[t], false);
            if s == ESCAPE {
                break;
            }
            dst[i] = s as i8;
            i += 1;
            if s == 0 {
                let mut len = huffman(&mut self.bits, &mut self.trees[0x100], true);
                if len == ESCAPE {
                    break;
                }
                len -= 1;
                if len != 0 {
                    self.length = len as u32;
                    let take = (len as usize).min(n - i);
                    self.length -= take as u32;
                    for v in &mut dst[i..i + take] {
                        *v = 0;
                    }
                    i += take;
                }
            }
            t = (s & 0xff) as usize;
        }
        self.last_tree = t;
        i
    }
}

fn walk(bits: &mut Bits, tree: &Tree) -> Option<(usize, i32)> {
    let mut child = tree.n[ROOT].child;
    loop {
        let b = bits.bit();
        if b == 1 {
            return None;
        }
        let entry = (child + (b & 1)) as usize;
        child = tree.n[entry].child;
        if child & CODE_FLAG != 0 {
            return Some((entry, child));
        }
    }
}

/// The next code of an adaptive Huffman tree; a new code follows an escape as 8 bits (run
/// lengths: as a gamma code).
fn huffman(bits: &mut Bits, tree: &mut Tree, length: bool) -> i32 {
    if tree.escape != NULL {
        let Some((entry, child)) = walk(bits, tree) else {
            return ESCAPE;
        };
        tree.increase(entry);
        let code = child & !CODE_FLAG;
        if code != ESCAPE {
            return code;
        }
    }
    let code = if length {
        gamma(bits)
    } else {
        bits.bits(8) as i32
    };
    tree.add(code);
    code
}

fn gamma(bits: &mut Bits) -> i32 {
    let b = bits.bit();
    if b == 1 {
        return 0;
    }
    if b == 0 {
        return 1;
    }
    let mut code: i32 = 0;
    let mut base: i32 = 2;
    loop {
        let c = bits.bit();
        if c == 1 {
            return 0;
        }
        code = (code << 1) | (c & 1);
        let more = bits.bit();
        if more == 1 {
            return 0;
        }
        if more == 0 {
            return code + base;
        }
        base <<= 1;
    }
}

struct Lossless {
    arch: i32,
    format: u32,
    bpp: u32,
    w: u32,
    h: u32,
    flip: bool,
    channels: usize,
    bs: usize,
    area: usize,
    samples: usize,
    wblocks: usize,
    hblocks: usize,
    table: Vec<usize>,
    huffman_ops: bool,
}

impl Lossless {
    fn new(i: &Info) -> Result<Lossless, String> {
        if i.blocking == 0 {
            return Err("eri: no blocking".into());
        }
        if i.arch != -1 && i.arch != -4 {
            return Err(format!("eri: code {} not supported", i.arch));
        }
        let channels = match i.format & 0xffff {
            1 => {
                if i.bpp <= 8 {
                    1
                } else if i.format & 0x0400_0000 == 0 {
                    3
                } else {
                    4
                }
            }
            2 => 1,
            f => return Err(format!("eri: format {f:#x} not supported")),
        };
        let bs = 1usize << i.blocking;
        let area = bs * bs;
        let samples = area * channels;
        let mut l = Lossless {
            arch: i.arch,
            format: i.format,
            bpp: i.bpp,
            w: i.w,
            h: i.h,
            flip: i.flip,
            channels,
            bs,
            area,
            samples,
            wblocks: (i.w as usize).div_ceil(bs),
            hblocks: (i.h as usize).div_ceil(bs),
            table: Vec::new(),
            huffman_ops: i.version == 0x0002_0200 && i.arch == -4,
        };
        l.arrange_table();
        Ok(l)
    }

    fn arrange_table(&mut self) {
        let (n, bs, ch, area) = (self.samples, self.bs, self.channels, self.area);
        let mut t = vec![0usize; n * 4];
        for (i, v) in t[..n].iter_mut().enumerate() {
            *v = i;
        }
        let mut p = n;
        let mut l = 0;
        for _ in 0..ch {
            for j in 0..bs {
                let mut m = l + j;
                for _ in 0..bs {
                    t[p] = m;
                    p += 1;
                    m += bs;
                }
            }
            l += area;
        }
        p = n * 2;
        for i in 0..area {
            let mut k = i;
            for _ in 0..ch {
                t[p] = k;
                p += 1;
                k += area;
            }
        }
        p = n * 3;
        for i in 0..bs {
            let mut l = i;
            for _ in 0..bs {
                let mut m = l;
                l += bs;
                for _ in 0..ch {
                    t[p] = m;
                    p += 1;
                    m += area;
                }
            }
        }
        self.table = t;
    }

    fn decode(&self, frame: &[u8]) -> Result<Vec<u8>, String> {
        let _ = self.huffman_ops;
        let mut c = Ctx {
            bits: Bits::new(frame),
            zero: 0,
            length: 0,
            trees: Vec::new(),
            last_tree: 0,
        };
        let version = c.bits.bits(8);
        let op_table = c.bits.bits(8);
        let enc = c.bits.bits(8);
        let bit_count = c.bits.bits(8);
        if op_table != 0 || enc & 0xfe != 0 {
            return Err("eri: bad stream header".into());
        }
        match version {
            1 if bit_count == 0 => {}
            8 if bit_count == 8 => {}
            16 if bit_count == 8 && enc == 0 => {}
            v => return Err(format!("eri: stream version {v} not supported")),
        }
        let gamma = self.arch == -1;
        let mut op_tree = Tree::new();
        let nblocks = self.wblocks * self.hblocks;
        let mut ops = Vec::new();
        if enc & 1 != 0 && self.channels >= 3 {
            for _ in 0..nblocks {
                if gamma {
                    ops.push(c.bits.bits(4) | 0xc0);
                } else {
                    ops.push(huffman(&mut c.bits, &mut op_tree, false) as u32);
                }
            }
        }
        if c.bits.bit() != 0 {
            return Err("eri: bad stream".into());
        }
        if gamma {
            if enc & 1 != 0 {
                c.init_gamma();
            }
        } else {
            c.prepare_erina();
        }
        let (bs, ch, area, n) = (self.bs, self.channels, self.area, self.samples);
        let wsamples = ch * self.wblocks * bs;
        let mut line = vec![0i8; wsamples];
        let mut column = vec![0i8; bs * ch];
        let mut arrange = vec![0i8; n];
        let mut buf = vec![0i8; n];
        let px_bytes = (self.bpp / 8).max(1) as usize;
        let stride = self.w as usize * 4;
        let mut out = vec![0u8; stride * self.h as usize];
        let mut opi = 0;
        let mut left_h = self.h as i64;
        for by in 0..self.hblocks {
            column.iter_mut().for_each(|v| *v = 0);
            let dh = (bs as i64).min(left_h) as usize;
            let mut left_w = self.w as i64;
            let mut lp = 0usize;
            for bx in 0..self.wblocks {
                let dw = (bs as i64).min(left_w) as usize;
                let op: u32 = if ch >= 3 {
                    if enc & 1 != 0 {
                        let v = ops[opi];
                        opi += 1;
                        v
                    } else if !gamma {
                        huffman(&mut c.bits, &mut op_tree, false) as u32
                    } else {
                        let v = c.bits.bits(4) | 0xc0;
                        c.init_gamma();
                        v
                    }
                } else {
                    if gamma && enc & 1 == 0 {
                        c.init_gamma();
                    }
                    if self.format == 2 { 0xc0 } else { 0 }
                };
                let got = if gamma {
                    c.gamma_bytes(&mut arrange)
                } else {
                    c.erina_bytes(&mut arrange)
                };
                if got < n {
                    return Err("eri: stream ends".into());
                }
                self.operate(op, &arrange, &mut buf, &mut column, &mut line, lp);
                lp += bs * ch;
                // restore into RGBA
                for y in 0..dh {
                    let iy = by * bs + y;
                    let oy = if self.flip {
                        iy
                    } else {
                        self.h as usize - 1 - iy
                    };
                    for x in 0..dw {
                        let s = y * bs + x;
                        let o = oy * stride + (bx * bs + x) * 4;
                        if ch == 1 {
                            let g = buf[s] as u8;
                            out[o..o + 4].copy_from_slice(&[g, g, g, g]);
                        } else {
                            let b = buf[s] as u8;
                            let g = buf[s + area] as u8;
                            let r = buf[s + area * 2] as u8;
                            let a = if ch == 4 {
                                buf[s + area * 3] as u8
                            } else {
                                255
                            };
                            out[o..o + 4].copy_from_slice(&[r, g, b, a]);
                        }
                    }
                }
                let _ = px_bytes;
                left_w -= bs as i64;
            }
            left_h -= bs as i64;
        }
        Ok(out)
    }

    fn operate(
        &self,
        op: u32,
        arrange: &[i8],
        buf: &mut [i8],
        column: &mut [i8],
        line: &mut [i8],
        mut lp: usize,
    ) {
        let (bs, ch, area, n) = (self.bs, self.channels, self.area, self.samples);
        let color = op & 0x0f;
        let arr = ((op >> 4) & 3) as usize;
        let diff = (op >> 6) & 3;
        if arr == 0 {
            buf.copy_from_slice(arrange);
            if op == 0 {
                return;
            }
        } else {
            let t = &self.table[arr * n..arr * n + n];
            for i in 0..n {
                buf[t[i]] = arrange[i];
            }
        }
        // color operations: (source channel, targets)
        let (src, dst): (usize, &[usize]) = match color {
            5 => (0, &[1]),
            6 => (0, &[2]),
            7 => (0, &[1, 2]),
            9 => (1, &[0]),
            10 => (1, &[2]),
            11 => (1, &[0, 2]),
            13 => (2, &[0]),
            14 => (2, &[1]),
            15 => (2, &[0, 1]),
            _ => (0, &[]),
        };
        if !dst.is_empty() {
            for i in 0..area {
                let base = buf[i + src * area];
                for &d in dst {
                    buf[i + d * area] = buf[i + d * area].wrapping_add(base);
                }
            }
        }
        let lines = bs * ch;
        if diff & 1 != 0 {
            let mut p = 0;
            for c in column.iter_mut().take(lines) {
                let mut last = *c;
                for _ in 0..bs {
                    last = last.wrapping_add(buf[p]);
                    buf[p] = last;
                    p += 1;
                }
                *c = last;
            }
        } else {
            for (i, c) in column.iter_mut().take(lines).enumerate() {
                *c = buf[i * bs + bs - 1];
            }
        }
        let mut d = 0usize;
        for _ in 0..ch {
            // the first row adds the line buffer, the rest the row above
            for j in 0..bs {
                buf[d + j] = buf[d + j].wrapping_add(line[lp + j]);
            }
            for _ in 1..bs {
                for j in 0..bs {
                    buf[d + bs + j] = buf[d + bs + j].wrapping_add(buf[d + j]);
                }
                d += bs;
            }
            line[lp..lp + bs].copy_from_slice(&buf[d..d + bs]);
            d += bs;
            lp += bs;
        }
    }
}
