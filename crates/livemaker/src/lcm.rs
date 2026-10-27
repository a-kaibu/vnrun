//! LiveCinema animations (.lcm): pictures moved, turned, scaled and faded over frames.
//!
//! The file is "LiveCinema" and a three-digit version, the canvas size (u32 each), a colour,
//! 13 bytes, the number of keys and the keys. A key is a kind byte (0 a picture) and a body,
//! then for a picture its file name (u32 length, Shift_JIS), found next to the .lcm.
//! A picture key shows its picture from `start` for `len` frames on a track (higher tracks in
//! front), moving the picture's pivot point from one place to another and its turn, scale
//! and transparency likewise, with an easing. The layout below is read from versions 108
//! and 113 (113 adds 4 bytes before the pivot and the end turn); fields not known are skipped.

/// LiveCinema plays 30 frames a second (a guess that matches the trials' timing).
pub const FPS: f64 = 30.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub picture: String,
    pub track: i32,
    pub start: u32,
    pub len: u32,
    /// 0 even, 1 speeding up, 2 slowing down
    pub ease: u8,
    /// the point of the picture that is placed and turned about
    pub pivot: (f64, f64),
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// degrees, clockwise
    pub turn: (f64, f64),
    pub scale_from: (f64, f64),
    pub scale_to: (f64, f64),
    /// opacity 0..=1 at the start and end
    pub alpha: (f64, f64),
    /// added to what is below rather than laid over it
    pub add: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cinema {
    pub width: u32,
    pub height: u32,
    pub keys: Vec<Key>,
}

/// Where a picture is at one moment: pivot placed at `at`, turned, scaled, with an opacity.
#[derive(Debug, Clone, PartialEq)]
pub struct Pose<'a> {
    pub picture: &'a str,
    pub pivot: (f64, f64),
    pub at: (f64, f64),
    pub turn: f64,
    pub scale: (f64, f64),
    pub alpha: f64,
    pub add: bool,
}

/// transparency (0 none .. 255, -1 hidden) to opacity
fn opacity(t: i32) -> f64 {
    if t < 0 {
        0.0
    } else {
        1.0 - t.min(255) as f64 / 255.0
    }
}

pub fn parse(b: &[u8]) -> Option<Cinema> {
    if !b.starts_with(b"LiveCinema") || b.len() < 42 {
        return None;
    }
    let version: u32 = std::str::from_utf8(b.get(10..13)?).ok()?.parse().ok()?;
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
    };
    let i32_at = |at: usize| u32_at(at).map(|v| v as i32);
    let f64_at = |at: usize| -> Option<f64> {
        Some(f64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
    };
    let (width, height, count) = (u32_at(13)?, u32_at(17)?, u32_at(38)?);
    // (body size after the kind byte, extra bytes before the pivot, end turn present)
    let (body, shift, two_turns) = if version >= 113 {
        (166, 4, true)
    } else {
        (138, 0, false)
    };
    let mut at = 42;
    let mut keys = Vec::new();
    for _ in 0..count.min(10000) {
        let kind = *b.get(at)?;
        let r = at;
        let end = r + 1 + body;
        if b.len() < end {
            break;
        }
        // Other kinds (2 is seen, a box of some sort) have bodies of other sizes, so the keys
        // after one cannot be found.
        if kind != 0 {
            break;
        }
        let n = u32_at(end)? as usize;
        let picture = nexas::sjis::decode(b.get(end + 4..end + 4 + n)?);
        let next = end + 4 + n;
        {
            let p = r + 28 + shift;
            let f = |o: usize| i32_at(p + o).unwrap_or(0) as f64;
            let ease_at = p + 24;
            let turn0 = f64_at(ease_at + 2).unwrap_or(0.0);
            let (turn1, after) = if two_turns {
                (f64_at(ease_at + 10).unwrap_or(0.0), ease_at + 18)
            } else {
                (turn0, ease_at + 10)
            };
            let sx0 = f64_at(after).unwrap_or(1.0);
            let sy0 = f64_at(after + 8).unwrap_or(1.0);
            let t0 = i32_at(after + 17).unwrap_or(0);
            let t1 = i32_at(after + 21).unwrap_or(0);
            let mode = i32_at(after + 25).unwrap_or(0);
            let sx1 = f64_at(after + 60).unwrap_or(1.0);
            let sy1 = f64_at(after + 68).unwrap_or(1.0);
            keys.push(Key {
                picture,
                track: i32_at(r + 9).unwrap_or(0),
                start: u32_at(r + 13).unwrap_or(0),
                len: u32_at(r + 5).unwrap_or(1),
                ease: b[ease_at],
                pivot: (f(0), f(4)),
                from: (f(8), f(12)),
                to: (f(16), f(20)),
                turn: (turn0, turn1),
                scale_from: (sx0, sy0),
                scale_to: (sx1, sy1),
                alpha: (opacity(t0), opacity(t1)),
                add: mode == 2,
            });
        }
        at = next;
    }
    Some(Cinema {
        width,
        height,
        keys,
    })
}

impl Cinema {
    /// The number of frames until the last key ends.
    pub fn frames(&self) -> u32 {
        self.keys.iter().map(|k| k.start + k.len).max().unwrap_or(0)
    }

    /// The pictures shown at a frame (counted from 0), back to front.
    pub fn poses(&self, frame: f64) -> Vec<Pose<'_>> {
        let last = self.frames() as f64;
        let frame = frame.clamp(0.0, (last - 1.0).max(0.0));
        let mut on: Vec<&Key> = self
            .keys
            .iter()
            .filter(|k| {
                let (s, e) = (k.start as f64, (k.start + k.len) as f64);
                // the last frame keeps showing keys that end with the cinema
                (s <= frame && frame < e) || (e >= last && frame >= last - 1.0 && s <= frame)
            })
            .collect();
        on.sort_by_key(|k| k.track);
        on.into_iter()
            .map(|k| {
                let t = if k.len <= 1 {
                    1.0
                } else {
                    ((frame - k.start as f64) / (k.len - 1) as f64).clamp(0.0, 1.0)
                };
                let t = match k.ease {
                    1 => t * t,
                    2 => 1.0 - (1.0 - t) * (1.0 - t),
                    _ => t,
                };
                let mix = |a: f64, b: f64| a + (b - a) * t;
                Pose {
                    picture: &k.picture,
                    pivot: k.pivot,
                    at: (mix(k.from.0, k.to.0), mix(k.from.1, k.to.1)),
                    turn: mix(k.turn.0, k.turn.1),
                    scale: (
                        mix(k.scale_from.0, k.scale_to.0),
                        mix(k.scale_from.1, k.scale_to.1),
                    ),
                    alpha: mix(k.alpha.0, k.alpha.1),
                    add: k.add,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version 113 cinema with one picture key sliding in and fading in.
    fn sample() -> Vec<u8> {
        let mut b = b"LiveCinema113".to_vec();
        b.extend(1280u32.to_le_bytes());
        b.extend(720u32.to_le_bytes());
        b.extend((-1i32).to_le_bytes());
        b.extend([0u8; 13]);
        b.extend(1u32.to_le_bytes());
        let mut r = vec![0u8; 167];
        r[1..5].copy_from_slice(&(-1i32).to_le_bytes());
        r[5..9].copy_from_slice(&11u32.to_le_bytes());
        r[9..13].copy_from_slice(&1u32.to_le_bytes());
        for (o, v) in [
            (40, -400i32),
            (44, 48),
            (48, 100),
            (52, 48),
            (91, -1),
            (95, 0),
        ] {
            r[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        for o in [74, 82, 134, 142] {
            r[o..o + 8].copy_from_slice(&1.0f64.to_le_bytes());
        }
        b.extend(r);
        b.extend(5u32.to_le_bytes());
        b.extend(b"a.gal");
        b
    }

    #[test]
    fn reads_and_plays_a_key() {
        let c = parse(&sample()).unwrap();
        assert_eq!((c.width, c.height, c.frames()), (1280, 720, 11));
        let k = &c.keys[0];
        assert_eq!(
            (k.picture.as_str(), k.from, k.to),
            ("a.gal", (-400.0, 48.0), (100.0, 48.0))
        );
        assert_eq!(k.alpha, (0.0, 1.0));
        let p = &c.poses(5.0)[0];
        assert_eq!(p.at, (-150.0, 48.0));
        assert!((p.alpha - 0.5).abs() < 1e-9);
        assert_eq!(c.poses(100.0)[0].at, (100.0, 48.0));
    }
}
