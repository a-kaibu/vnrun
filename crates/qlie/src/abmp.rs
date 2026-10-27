//! ABMP (.b) files: animated pictures. An "abmp1x" file holds the animation ("abdata", 1PC
//! packed), the pictures it shows ("abimage10": PNG, BMP or JPEG files) and its sounds
//! ("absound10"). The animation is a list of motions ("*Enable", "*OnMouse", "Click", ...),
//! each a list of layers, each a list of keys: at a time, a picture (or a coloured box) at a
//! place, scaled, turned, tinted and faded, maybe moving on smoothly to the next key, maybe with
//! a command (go back to an earlier key, play a sound).

/// What a layer shows from a key on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Show {
    /// nothing
    None,
    /// a box of `color`, `sx` by `sy` pixels
    Fill,
    /// picture `image`, scaled `sx`/`sy` percent
    Image,
    /// where the pointer counts as on the object (`sx` by `sy`)
    Hit,
    Other(u8),
}

#[derive(Debug, Clone)]
pub struct Key {
    pub time: u32,
    pub show: Show,
    pub image: u16,
    /// the centre of the picture, from the object's place
    pub x: f32,
    pub y: f32,
    pub sx: f32,
    pub sy: f32,
    pub z: i16,
    /// degrees, clockwise
    pub rot: f32,
    /// added to the picture's colour
    pub add: [u8; 3],
    /// the picture's colour is multiplied by this (the box's colour)
    pub color: [u8; 3],
    pub alpha: f32,
    /// how it moves on to the next key: 0 not (it stays until then), 1 evenly, 3 slowing
    /// down, 4 speeding up
    pub ease: u8,
    pub tween: bool,
    /// which point of the picture is at x, y: 0 top left, 1 top, 2 top right, 3 left, 4 centre,
    /// ... 8 bottom right
    pub anchor: u8,
    /// 1: go to key `arg`; 7: stop; 8: play sound `arg`
    pub cmd: u8,
    pub arg: u8,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    pub keys: Vec<Key>,
}

#[derive(Debug, Clone)]
pub struct Motion {
    pub name: String,
    pub layers: Vec<Layer>,
}

#[derive(Debug, Default)]
pub struct Abmp {
    /// the files of the pictures (PNG, BMP, JPEG, ...), by number
    pub images: Vec<Vec<u8>>,
    pub sounds: Vec<Vec<u8>>,
    pub motions: Vec<Motion>,
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn tag(&mut self) -> Option<String> {
        let t = self.take(16)?;
        let n = t.iter().position(|&c| c == 0).unwrap_or(16);
        Some(String::from_utf8_lossy(&t[..n]).into_owned())
    }
}

fn sjis(b: &[u8]) -> String {
    crate::script::decode_sjis(b)
}

impl Abmp {
    pub fn parse(b: &[u8]) -> Result<Abmp, String> {
        if b.starts_with(b"ABMP7") {
            return Ok(parse7(b));
        }
        if !b.starts_with(b"abmp") {
            return Err("not an ABMP file".into());
        }
        let mut a = Abmp::default();
        let mut r = Rd { b, p: 16 };
        while r.p + 16 <= b.len() {
            let tag = r.tag().ok_or("truncated")?;
            if tag.starts_with("abdata") {
                let n = r.u32().ok_or("truncated")? as usize;
                let data = r.take(n).ok_or("truncated abdata")?;
                let data = if data.starts_with(b"1PC\xff") {
                    crate::pack::decompress(data).ok_or("abdata: bad 1PC")?
                } else {
                    data.to_vec()
                };
                a.motions = parse_motions(&data);
            } else if tag == "abimage10" || tag == "absound10" {
                let count = r.u8().ok_or("truncated")?;
                for _ in 0..count {
                    let t = r.tag().ok_or("truncated")?;
                    if t == "abimgdat15" {
                        let ver = r.u32().ok_or("truncated")?;
                        let n = r.u16().ok_or("truncated")? as usize;
                        r.take(n * 2).ok_or("truncated")?;
                        let n = r.u16().ok_or("truncated")? as usize;
                        r.take(n).ok_or("truncated")?;
                        r.u8().ok_or("truncated")?;
                        r.take(if ver == 2 { 0x1d } else { 0x11 })
                            .ok_or("truncated")?;
                    } else if t == "absnddat12" {
                        r.u32().ok_or("truncated")?;
                        let n = r.u16().ok_or("truncated")? as usize;
                        r.take(n * 2).ok_or("truncated")?;
                        if b.len() - r.p <= 7 {
                            break;
                        }
                        r.take(7).ok_or("truncated")?;
                    } else {
                        let n = r.u16().ok_or("truncated")? as usize;
                        r.take(n).ok_or("truncated")?;
                        if t != "abimgdat10" && t != "absnddat10" {
                            let n = r.u16().ok_or("truncated")? as usize;
                            r.take(n).ok_or("truncated")?;
                            if t == "abimgdat13" {
                                r.take(12).ok_or("truncated")?;
                            } else if t == "abimgdat14" {
                                r.take(0x4c).ok_or("truncated")?;
                            }
                        }
                        r.u8().ok_or("truncated")?;
                    }
                    let size = r.u32().ok_or("truncated")? as usize;
                    let data = r.take(size).ok_or("truncated picture")?;
                    let data = if data.starts_with(b"1PC\xff") {
                        crate::pack::decompress(data).unwrap_or_default()
                    } else {
                        data.to_vec()
                    };
                    if tag == "abimage10" {
                        a.images.push(data);
                    } else {
                        a.sounds.push(data);
                    }
                }
            } else {
                break;
            }
        }
        Ok(a)
    }

    pub fn motion(&self, name: &str) -> Option<usize> {
        let name = name.trim_start_matches('*');
        self.motions
            .iter()
            .position(|m| m.name.trim_start_matches('*').eq_ignore_ascii_case(name))
    }

    /// How long a motion runs before it stops or holds (the last key).
    pub fn length(&self, m: usize) -> u32 {
        self.motions.get(m).map_or(0, |m| {
            m.layers
                .iter()
                .filter_map(|l| l.keys.last().map(|k| k.time))
                .max()
                .unwrap_or(0)
        })
    }
}

/// The older "ABMP7": animation data (not read), then the pictures, each after its size.
fn parse7(b: &[u8]) -> Abmp {
    let mut a = Abmp::default();
    let rd = |o: usize| {
        b.get(o..o + 4)
            .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
    };
    let mut at = 0xc;
    let Some(n) = rd(at) else { return a };
    at += 4 + n as usize;
    while let Some(n) = rd(at) {
        if n == 0 {
            break;
        }
        at += 4;
        let Some(d) = b.get(at..at + n as usize) else {
            break;
        };
        a.images.push(if d.starts_with(b"1PC\xff") {
            crate::pack::decompress(d).unwrap_or_default()
        } else {
            d.to_vec()
        });
        at += n as usize;
    }
    a
}

fn find(d: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    d.get(from..)?
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|i| i + from)
}

/// The motions of the animation data: "abmotion" blocks of "ablayer" blocks of keys.
fn parse_motions(d: &[u8]) -> Vec<Motion> {
    let mut out = Vec::new();
    let mut at = find(d, b"abmotion", 0);
    while let Some(m) = at {
        let mut r = Rd { b: d, p: m + 16 };
        let Some(name) = r.u16().and_then(|n| r.take(n as usize)).map(sjis) else {
            break;
        };
        let Some(nlayers) = r.u8() else { break };
        let mut layers = Vec::new();
        for _ in 0..nlayers {
            if d.get(r.p..r.p + 7) != Some(b"ablayer") {
                break;
            }
            r.p += 16;
            let Some(lname) = r.u16().and_then(|n| r.take(n as usize)).map(sjis) else {
                break;
            };
            let Some(count) = r.u16() else { break };
            // the size of a key: the room up to the next block, shared out
            let next = [
                find(d, b"ablayer", r.p),
                find(d, b"abmotion", r.p),
                Some(d.len()),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap();
            let size = if count == 0 {
                0
            } else {
                (next - r.p) / count as usize
            };
            let mut keys = Vec::new();
            for _ in 0..count {
                let Some(k) = r.take(size) else { break };
                if let Some(k) = parse_key(k) {
                    keys.push(k);
                }
            }
            layers.push(Layer { name: lname, keys });
        }
        out.push(Motion { name, layers });
        at = find(d, b"abmotion", r.p);
    }
    out
}

fn parse_key(k: &[u8]) -> Option<Key> {
    if k.len() < 4 + 54 {
        return None;
    }
    let i16at = |j: usize| i16::from_le_bytes([k[4 + j], k[5 + j]]);
    let j = |n: usize| k[4 + n];
    let show = match j(0) {
        0 => Show::None,
        1 => Show::Fill,
        4 => Show::Image,
        6 => Show::Hit,
        n => Show::Other(n),
    };
    Some(Key {
        time: u32::from_le_bytes(k[0..4].try_into().ok()?),
        show,
        image: u16::from_le_bytes([j(2), j(3)]),
        x: i16at(20) as f32,
        y: i16at(22) as f32,
        sx: i16at(24) as f32,
        sy: i16at(26) as f32,
        z: i16at(28),
        rot: i16at(30) as f32,
        add: [j(32), j(33), j(34)],
        color: [j(36), j(37), j(38)],
        alpha: j(40) as f32 / 255.0,
        ease: j(44),
        tween: j(44) != 0,
        anchor: j(46),
        cmd: j(48),
        arg: j(49),
    })
}

/// Where a layer is in its keys.
#[derive(Debug, Clone, Copy, Default)]
pub struct Head {
    pub t: f32,
    /// the key last passed
    pub key: usize,
    pub stopped: bool,
    /// it went back to an earlier key at least once
    pub looped: bool,
    /// a key asked to go on with another motion (command 5)
    pub goto: Option<u8>,
}

/// What a layer shows now.
#[derive(Debug, Clone, Copy)]
pub struct State {
    pub show: Show,
    pub image: u16,
    pub x: f32,
    pub y: f32,
    pub sx: f32,
    pub sy: f32,
    pub z: i16,
    pub rot: f32,
    pub add: [u8; 3],
    pub color: [u8; 3],
    pub alpha: f32,
    pub anchor: u8,
}

fn ease(e: u8, f: f32) -> f32 {
    match e {
        3 => 1.0 - (1.0 - f) * (1.0 - f),
        4 => f * f,
        _ => f,
    }
}

impl Layer {
    /// Moves the head on `dt` ms; the sounds whose keys it passed are added to `sounds`.
    pub fn advance(&self, h: &mut Head, dt: f32, sounds: &mut Vec<u8>) {
        if h.stopped || self.keys.is_empty() {
            return;
        }
        h.t += dt;
        // pass the keys up to t, with their commands; a jump back counts as a pass
        let mut guard = 0;
        while guard < 64 {
            guard += 1;
            let Some(next) = self.keys.get(h.key + 1) else {
                break;
            };
            if (next.time as f32) > h.t {
                break;
            }
            h.key += 1;
            match next.cmd {
                1 => {
                    let to = next.arg as usize;
                    if to >= h.key || self.keys[to].time >= next.time {
                        h.stopped = true;
                        h.t = next.time as f32;
                        return;
                    }
                    let over = h.t - next.time as f32;
                    h.looped = true;
                    h.key = to;
                    h.t = self.keys[to].time as f32 + over;
                }
                7 => {
                    h.stopped = true;
                    h.t = next.time as f32;
                    return;
                }
                5 => {
                    h.stopped = true;
                    h.t = next.time as f32;
                    h.goto = Some(next.arg);
                    return;
                }
                8 => sounds.push(next.arg),
                _ => {}
            }
        }
    }

    /// Starts the layer over: the head at 0, and the first key's sound.
    pub fn start(&self, h: &mut Head, sounds: &mut Vec<u8>) {
        *h = Head::default();
        if let Some(k) = self.keys.first()
            && k.cmd == 8
            && k.time == 0
        {
            sounds.push(k.arg);
        }
    }

    pub fn state(&self, h: &Head) -> Option<State> {
        let k = self.keys.get(h.key)?;
        if h.t < k.time as f32 && h.key == 0 {
            return None;
        }
        let mut s = State {
            show: k.show,
            image: k.image,
            x: k.x,
            y: k.y,
            sx: k.sx,
            sy: k.sy,
            z: k.z,
            rot: k.rot,
            add: k.add,
            color: k.color,
            alpha: k.alpha,
            anchor: k.anchor,
        };
        if k.tween
            && !h.stopped
            && let Some(n) = self.keys.get(h.key + 1)
            && n.time > k.time
        {
            let f = ((h.t - k.time as f32) / (n.time - k.time) as f32).clamp(0.0, 1.0);
            let f = ease(k.ease, f);
            let l = |a: f32, b: f32| a + (b - a) * f;
            s.x = l(k.x, n.x);
            s.y = l(k.y, n.y);
            s.sx = l(k.sx, n.sx);
            s.sy = l(k.sy, n.sy);
            s.rot = l(k.rot, n.rot);
            s.alpha = l(k.alpha, n.alpha);
            for c in 0..3 {
                s.add[c] = l(k.add[c] as f32, n.add[c] as f32) as u8;
                s.color[c] = l(k.color[c] as f32, n.color[c] as f32) as u8;
            }
        }
        Some(s)
    }
}
