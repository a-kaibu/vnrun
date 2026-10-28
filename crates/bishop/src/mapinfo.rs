//! map.dat ("BSX MAP INFO r3", BSScript Rev.6): what the map phase shows. A condition (a
//! global and a number), the names of the map's icons (map_pNN places, map_chXY characters:
//! X the heroine, the second half of each 100x200 picture the lit one), anchors (an area
//! kind, a point, and where 1, 2, 3 or 4 icons go from it: 1+2+3+4 offsets), the heroines
//! (their progress and place globals, then a record whose bytes 6 and 7 are the area kind and
//! the icon index), and the global the choice goes into.

pub struct Anchor {
    pub kind: u8,
    pub x: i32,
    pub y: i32,
    /// offsets for 1, 2, 3 and 4 icons, one after another
    pub offsets: Vec<(i32, i32)>,
}

pub struct Heroine {
    /// e.g. "#est_progres"
    pub progress: String,
    pub place: String,
    pub area: u8,
    pub icon: usize,
}

pub struct MapInfo {
    pub names: Vec<String>,
    pub anchors: Vec<Anchor>,
    pub heroines: Vec<Heroine>,
    pub result: String,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let v = u16::from_le_bytes(self.b.get(self.p..self.p + 2)?.try_into().ok()?);
        self.p += 2;
        Some(v)
    }
    fn i32(&mut self) -> Option<i32> {
        let v = i32::from_le_bytes(self.b.get(self.p..self.p + 4)?.try_into().ok()?);
        self.p += 4;
        Some(v)
    }
    fn cstr(&mut self) -> Option<String> {
        let e = self.b.get(self.p..)?.iter().position(|&c| c == 0)?;
        let s = nexas::sjis::decode(&self.b[self.p..self.p + e]);
        self.p += e + 1;
        Some(s)
    }
    fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.p..self.p + n)?;
        self.p += n;
        Some(s)
    }
}

impl MapInfo {
    pub fn parse(b: &[u8]) -> Option<MapInfo> {
        if !b.starts_with(b"BSX MAP INFO") {
            return None;
        }
        let mut r = Reader { b, p: 16 };
        // the condition: a global, a kind and a number
        r.cstr()?;
        r.u8()?;
        r.i32()?;
        let n = r.u8()? as usize;
        let names = (0..n).map(|_| r.cstr()).collect::<Option<Vec<_>>>()?;
        let n = r.u8()? as usize;
        let mut anchors = Vec::with_capacity(n);
        for _ in 0..n {
            let (kind, x, y, k) = (r.u8()?, r.u16()? as i32, r.u16()? as i32, r.u8()? as usize);
            let offsets = (0..k)
                .map(|_| Some((r.i32()?, r.i32()?)))
                .collect::<Option<Vec<_>>>()?;
            anchors.push(Anchor {
                kind,
                x,
                y,
                offsets,
            });
        }
        let n = r.u8()? as usize;
        let mut heroines = Vec::with_capacity(n);
        for _ in 0..n {
            let progress = r.cstr()?;
            let place = r.cstr()?;
            let rec = r.bytes(20)?;
            heroines.push(Heroine {
                progress,
                place,
                area: rec[6],
                icon: rec[7] as usize,
            });
        }
        r.u8()?;
        let result = r.cstr()?;
        Some(MapInfo {
            names,
            anchors,
            heroines,
            result,
        })
    }

    /// The heroine whose events are `<prefix>_ev..` ("est_ev11": the one of "#est_progres").
    pub fn heroine_of(&self, label: &str) -> Option<&Heroine> {
        let prefix = label.split('_').next()?;
        self.heroines.iter().find(|h| {
            h.progress
                .trim_start_matches('#')
                .split('_')
                .next()
                .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
        })
    }

    /// Where the `i`th of `n` icons of an area goes (its top left), from the area's first anchor.
    pub fn place(&self, area: u8, i: usize, n: usize) -> Option<(i32, i32)> {
        let a = self.anchors.iter().find(|a| a.kind == area)?;
        let n = n.clamp(1, 4);
        let (dx, dy) = a
            .offsets
            .get(n * (n - 1) / 2 + i.min(n - 1))
            .copied()
            .unwrap_or((0, 0));
        Some((a.x + dx, a.y + dy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        let mut b = b"BSX MAP INFO r3\0#time\0\x01\x06\0\0\0\x02map_p01\0map_ch37\0\x01".to_vec();
        b.extend([4, 200, 0, 250, 0, 3]);
        for (x, y) in [(0i32, 0i32), (0, 0), (-100, 0)] {
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
        }
        b.push(1);
        b.extend(b"#est_progres\0#est_map\0");
        b.extend([3, 0, 0, 0, 0, 1, 4, 1, 5, 0, 0, 0, 1, 0, 0, 100, 0, 0, 0, 0]);
        b.extend(b"\x01#result\0");
        let m = MapInfo::parse(&b).unwrap();
        assert_eq!(m.names, ["map_p01", "map_ch37"]);
        assert_eq!(m.result, "#result");
        let h = m.heroine_of("est_ev11").unwrap();
        assert_eq!((h.area, h.icon), (4, 1));
        assert!(m.heroine_of("mly_ev12").is_none());
        assert_eq!(m.place(4, 0, 1), Some((200, 250)));
        assert_eq!(m.place(4, 1, 2), Some((100, 250)));
    }
}
