//! What a save slot holds, in our own format: where the script was, the variables, what was on
//! the screen and playing, and a small picture of the screen.

/// A character on the screen: its slot, its picture name and where it stands.
pub struct SavedChara {
    pub slot: u8,
    pub name: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Default)]
pub struct SaveData {
    /// the script and the command to run again (the line or choice that was showing)
    pub script: String,
    pub pc: u32,
    pub stack: Vec<(String, u32)>,
    pub vars: Vec<(u8, u16, i32)>,
    pub bg: String,
    pub chars: Vec<SavedChara>,
    pub bgm: String,
    /// the scene's title and the line, shown in the slot
    pub title: String,
    pub text: String,
    /// the screen, small: width, height and RGBA
    pub thumb: (u32, u32, Vec<u8>),
}

const MAGIC: &[u8; 8] = b"TMRSAVE1";

impl SaveData {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(MAGIC.to_vec());
        w.str(&self.script);
        w.u32(self.pc);
        w.u32(self.stack.len() as u32);
        for (s, pc) in &self.stack {
            w.str(s);
            w.u32(*pc);
        }
        w.u32(self.vars.len() as u32);
        for &(bank, n, v) in &self.vars {
            w.u32(bank as u32);
            w.u32(n as u32);
            w.u32(v as u32);
        }
        w.str(&self.bg);
        w.u32(self.chars.len() as u32);
        for c in &self.chars {
            w.u32(c.slot as u32);
            w.str(&c.name);
            w.u32(c.x.to_bits());
            w.u32(c.y.to_bits());
        }
        w.str(&self.bgm);
        w.str(&self.title);
        w.str(&self.text);
        w.u32(self.thumb.0);
        w.u32(self.thumb.1);
        w.bytes(&self.thumb.2);
        w.0
    }

    pub fn decode(b: &[u8]) -> Option<SaveData> {
        let mut r = Reader { b, at: 0 };
        if r.take(8)? != MAGIC {
            return None;
        }
        let mut d = SaveData {
            script: r.str()?,
            pc: r.u32()?,
            ..SaveData::default()
        };
        for _ in 0..r.u32()? {
            d.stack.push((r.str()?, r.u32()?));
        }
        for _ in 0..r.u32()? {
            d.vars
                .push((r.u32()? as u8, r.u32()? as u16, r.u32()? as i32));
        }
        d.bg = r.str()?;
        for _ in 0..r.u32()? {
            d.chars.push(SavedChara {
                slot: r.u32()? as u8,
                name: r.str()?,
                x: f32::from_bits(r.u32()?),
                y: f32::from_bits(r.u32()?),
            });
        }
        d.bgm = r.str()?;
        d.title = r.str()?;
        d.text = r.str()?;
        d.thumb = (r.u32()?, r.u32()?, r.bytes()?);
        Some(d)
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        Some(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let d = SaveData {
            script: "fh00".into(),
            pc: 12,
            stack: vec![("start".into(), 40)],
            vars: vec![(1, 3, -5)],
            bg: "b001".into(),
            chars: vec![SavedChara {
                slot: 2,
                name: "a1210".into(),
                x: 300.0,
                y: 0.0,
            }],
            bgm: "m01".into(),
            title: "復讐".into(),
            text: "line".into(),
            thumb: (1, 1, vec![1, 2, 3, 4]),
        };
        let e = SaveData::decode(&d.encode()).unwrap();
        assert_eq!(e.script, "fh00");
        assert_eq!(e.stack, d.stack);
        assert_eq!(e.vars, d.vars);
        assert_eq!(e.chars[0].name, "a1210");
        assert_eq!(e.chars[0].x, 300.0);
        assert_eq!(e.thumb.2, vec![1, 2, 3, 4]);
    }
}
