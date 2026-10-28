//! What the 2018 and later programs keep to themselves: the title buttons. The program builds
//! each screen's buttons from a table in .data, named by a string ("Title"):
//!
//! ```text
//!     push "Title"; lea ecx, [edi+...]; call open_group
//!     mov esi, table; loop: push esi; call add_button; add esi, 0x84; cmp esi, table_end; jl loop
//! ```
//!
//! A button is 33 i32: id, the place on the screen (x0, y0, x1, y1), 0, the chip sheet of each
//! of its three states, the part of the sheet of each state (x0, y0, x1, y1: plain, lit,
//! pressed), then zeros.

/// A button of the program's table.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub id: i32,
    pub dst: [i32; 4],
    /// plain, lit, pressed
    pub src: [[i32; 4]; 3],
}

const ENTRY: usize = 33 * 4;

struct Pe<'a> {
    b: &'a [u8],
    /// (virtual address, file offset, size in the file, executable)
    sections: Vec<(u32, usize, usize, bool)>,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

impl<'a> Pe<'a> {
    fn parse(b: &'a [u8]) -> Option<Pe<'a>> {
        let pe = u32_at(b, 0x3c)? as usize;
        if b.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let n = u16_at(b, pe + 6)? as usize;
        let opt = u16_at(b, pe + 20)? as usize;
        let base = u32_at(b, pe + 24 + 28)?;
        let mut sections = Vec::new();
        for i in 0..n {
            let o = pe + 24 + opt + 40 * i;
            let va = u32_at(b, o + 12)?;
            let raw_size = u32_at(b, o + 16)? as usize;
            let raw = u32_at(b, o + 20)? as usize;
            let flags = u32_at(b, o + 36)?;
            sections.push((base.wrapping_add(va), raw, raw_size, flags & 0x20 != 0));
        }
        Some(Pe { b, sections })
    }

    fn offset(&self, va: u32) -> Option<usize> {
        self.sections.iter().find_map(|&(v, raw, size, _)| {
            (va >= v && ((va - v) as usize) < size).then(|| raw + (va - v) as usize)
        })
    }

    fn va(&self, off: usize) -> Option<u32> {
        self.sections.iter().find_map(|&(v, raw, size, _)| {
            (off >= raw && off < raw + size).then(|| v + (off - raw) as u32)
        })
    }

    fn code(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.sections
            .iter()
            .filter(|s| s.3)
            .map(|&(_, raw, size, _)| (raw, (raw + size).min(self.b.len())))
    }
}

fn entry(b: &[u8], o: usize) -> Option<Button> {
    let d: Vec<i32> = (0..33)
        .map(|k| u32_at(b, o + 4 * k).map(|v| v as i32))
        .collect::<Option<_>>()?;
    let rect = |k: usize| [d[k], d[k + 1], d[k + 2], d[k + 3]];
    let fine = |r: [i32; 4]| {
        0 <= r[0] && r[0] < r[2] && r[2] <= 4096 && 0 <= r[1] && r[1] < r[3] && r[3] <= 4096
    };
    let src = [rect(9), rect(13), rect(17)];
    let ok = (0..=64).contains(&d[0])
        && fine(rect(1))
        && d[6] == d[7]
        && d[7] == d[8]
        && (0..256).contains(&d[6])
        && src.iter().all(|r| fine(*r))
        && d[21..].iter().all(|&v| v == 0);
    ok.then(|| Button {
        id: d[0],
        dst: rect(1),
        src,
    })
}

/// The buttons of the screen the program calls `name` ("Title"), in table order.
pub fn buttons(exe: &[u8], name: &str) -> Option<Vec<Button>> {
    let pe = Pe::parse(exe)?;
    let mut key = name.as_bytes().to_vec();
    key.push(0);
    // the name as a string of its own (a 0 before it), then the code that pushes it
    let names: Vec<u32> = exe
        .windows(key.len())
        .enumerate()
        .filter(|(o, w)| *w == key.as_slice() && *o > 0 && exe[o - 1] == 0)
        .filter_map(|(o, _)| pe.va(o))
        .collect();
    for (start, end) in pe.code() {
        let code = &exe[start..end];
        for at in 0..code.len().saturating_sub(5) {
            if code[at] != 0x68 {
                continue;
            }
            let v = u32::from_le_bytes(code[at + 1..at + 5].try_into().unwrap());
            if !names.contains(&v) {
                continue;
            }
            if let Some(list) = table_after(&pe, &code[at + 5..(at + 5 + 64).min(code.len())]) {
                return Some(list);
            }
        }
    }
    None
}

/// Right after the call that opens the group: `mov r32, table` (b8..bf), and the `cmp r32,
/// end` (81 f8..ff) of the loop when there is one.
fn table_after(pe: &Pe, code: &[u8]) -> Option<Vec<Button>> {
    let call = (0..code.len().saturating_sub(10)).find(|&k| code[k] == 0xe8)?;
    let at = call + 5;
    if !(0xb8..=0xbf).contains(&code[at]) {
        return None;
    }
    let reg = code[at] - 0xb8;
    let table = u32::from_le_bytes(code[at + 1..at + 5].try_into().ok()?);
    let end = (at + 5..code.len().saturating_sub(6))
        .find(|&k| code[k] == 0x81 && code[k + 1] == 0xf8 + reg)
        .map(|k| u32::from_le_bytes(code[k + 2..k + 6].try_into().unwrap()));
    let first = pe.offset(table)?;
    let count = match end {
        Some(e) if e > table && ((e - table) as usize).is_multiple_of(ENTRY) => {
            (e - table) as usize / ENTRY
        }
        _ => 64,
    };
    let list: Vec<Button> = (0..count)
        .map_while(|k| entry(pe.b, first + k * ENTRY))
        .collect();
    (!list.is_empty()).then_some(list)
}
