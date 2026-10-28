//! Our own save slots: a text file per slot, one `key<TAB>values` line each. A slot holds the
//! page that was showing (the script and the item after its `<KW>`, the calls under it), the
//! variables, the pictures by name, the face, the window settings, the text and the music, so a
//! load shows that page again and waits for a click.

pub const SLOTS: usize = 12;

pub fn slot_name(k: usize) -> String {
    format!("aghsave{k:02}.txt")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveData {
    pub script: String,
    pub pc: usize,
    pub calls: Vec<(String, usize)>,
    pub vars: Vec<(String, i32)>,
    /// layer, picture, x, y
    pub layers: Vec<(i32, String, i32, i32)>,
    pub face: String,
    pub frame_on: bool,
    /// window, x, y, home x, home y, r, g, b, size
    pub wins: Vec<[i32; 9]>,
    pub name: String,
    pub lines: Vec<String>,
    pub bgm: String,
    pub loops: Vec<(i32, bool)>,
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

impl SaveData {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = String::from("aghsave\t1\n");
        o += &format!("script\t{}\t{}\n", clean(&self.script), self.pc);
        for (s, pc) in &self.calls {
            o += &format!("call\t{}\t{pc}\n", clean(s));
        }
        for (k, v) in &self.vars {
            o += &format!("var\t{}\t{v}\n", clean(k));
        }
        for (n, img, x, y) in &self.layers {
            o += &format!("layer\t{n}\t{}\t{x}\t{y}\n", clean(img));
        }
        o += &format!("face\t{}\n", clean(&self.face));
        o += &format!("frame\t{}\n", self.frame_on as i32);
        for w in &self.wins {
            let v: Vec<String> = w.iter().map(|x| x.to_string()).collect();
            o += &format!("win\t{}\n", v.join("\t"));
        }
        o += &format!("name\t{}\n", clean(&self.name));
        for l in &self.lines {
            o += &format!("line\t{}\n", clean(l));
        }
        o += &format!("bgm\t{}\n", clean(&self.bgm));
        for (ch, on) in &self.loops {
            o += &format!("loop\t{ch}\t{}\n", *on as i32);
        }
        o.into_bytes()
    }

    pub fn decode(b: &[u8]) -> Option<SaveData> {
        let t = std::str::from_utf8(b).ok()?;
        let mut lines = t.lines();
        if !lines.next()?.starts_with("aghsave\t") {
            return None;
        }
        let mut d = SaveData::default();
        for l in lines {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
            let s = |i: usize| f.get(i).copied().unwrap_or("").to_string();
            match f[0] {
                "script" => (d.script, d.pc) = (s(1), n(2).max(0) as usize),
                "call" => d.calls.push((s(1), n(2).max(0) as usize)),
                "var" => d.vars.push((s(1), n(2))),
                "layer" => d.layers.push((n(1), s(2), n(3), n(4))),
                "face" => d.face = s(1),
                "frame" => d.frame_on = n(1) != 0,
                "win" => d.wins.push(std::array::from_fn(|i| n(i + 1))),
                "name" => d.name = s(1),
                "line" => d.lines.push(s(1)),
                "bgm" => d.bgm = s(1),
                "loop" => d.loops.push((n(1), n(2) != 0)),
                _ => {}
            }
        }
        (!d.script.is_empty()).then_some(d)
    }

    /// What the slot shows: the speaker and the start of the text.
    pub fn summary(&self) -> String {
        let text: String = self
            .lines
            .join("")
            .replace('\u{3000}', "")
            .chars()
            .take(20)
            .collect();
        if self.name.is_empty() {
            text
        } else {
            format!("{}「{}", self.name, text.trim_start_matches('「'))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let d = SaveData {
            script: "A010.txt".into(),
            pc: 42,
            calls: vec![("root_tr.txt".into(), 7)],
            vars: vec![("flg_S001A".into(), 1)],
            layers: vec![
                (0, "bg002a.bmp".into(), 0, 0),
                (5, "you_f0_h1_mid.bmp".into(), 256, 0),
            ],
            face: "yuu_006.bmp".into(),
            frame_on: true,
            wins: vec![[0, 0, 496, 264, 80, 255, 255, 255, 24]],
            name: "裕人".into(),
            lines: vec!["「……ふうん。".into(), "　雲にも".into()],
            bgm: "bgm03.ogg".into(),
            loops: vec![(0, true), (2, false)],
        };
        assert_eq!(SaveData::decode(&d.encode()), Some(d));
    }
}
