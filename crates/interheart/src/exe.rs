//! What the 2018 and later programs keep to themselves: the title buttons, the choices and the
//! message window (`message_window`, below). The program builds each screen's buttons from a
//! table in .data, named by a string ("Title"):
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
            if let Some(list) = table_after(&pe, &code[at + 5..(at + 5 + 0xa0).min(code.len())]) {
                return Some(list);
            }
        }
    }
    None
}

/// Right after the call that opens the group: `mov r32, table` (b8..bf), and the `cmp r32,
/// end` (81 f8..ff) of the loop when there is one; or, for a table read by a count
/// ("SELECT"), `imul eax, eax, 0x84; add eax, table`.
fn table_after(pe: &Pe, code: &[u8]) -> Option<Vec<Button>> {
    let call = (0..code.len().saturating_sub(12)).find(|&k| code[k] == 0xe8)?;
    let mut at = call + 5;
    // a counter cleared first ("Message": xor edi, edi)
    if matches!(code[at], 0x31 | 0x33) && code[at + 1] >= 0xc0 {
        at += 2;
    }
    let (table, end) = if (0xb8..=0xbf).contains(&code[at]) {
        let reg = code[at] - 0xb8;
        let end = (at + 5..code.len().saturating_sub(6))
            .find(|&k| code[k] == 0x81 && code[k + 1] == 0xf8 + reg)
            .and_then(|k| u32_at(code, k + 2));
        (u32_at(code, at + 1)?, end)
    } else {
        let k = (at..code.len().saturating_sub(11))
            .find(|&k| code[k..k + 6] == [0x69, 0xc0, 0x84, 0, 0, 0] && code[k + 6] == 0x05)?;
        (u32_at(code, k + 7)?, None)
    };
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

/// The bars of the choices (2018 on): the "SELECT" table holds one button for each place, from
/// the top (as many as the most choices), each with the bar plain and lit.
pub fn choices(exe: &[u8]) -> Option<Vec<Button>> {
    let list = buttons(exe, "SELECT")?;
    let first = list.first()?.id;
    // a second layout of the same ids follows (another mode of the program)
    let n = (0..list.len())
        .take_while(|&k| list[k].id == first + k as i32)
        .count();
    Some(list[..n].to_vec())
}

/// A part of MesChip.kg and where the program draws it (both x0, y0, x1, y1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub src: [i32; 4],
    pub dst: [i32; 4],
}

/// The message window of a 2018 and later program.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// the window, then its end to the right of it (the sheet is narrower than the screen)
    pub parts: Vec<Part>,
    /// the name plate, drawn with a name
    pub plate: Part,
    /// where the first line of the text starts
    pub text: [i32; 2],
    /// the middle of the name and its top
    pub name: [i32; 2],
}

/// Where the program draws its message window. The parts are rects in .data:
///
/// ```text
///     window: 0, 0, w, h | +0x10 its end (or empty) | +0x20, +0x30 others | +0x40 the name plate
/// ```
///
/// each drawn by `push 1; push part; lea eax, [ebp+dst]; push eax; call draw`, the place made
/// from globals just before (`mov r32, [g]`, `add`/`sub`, `mov [ebp+d], r32`). The text and the
/// name are set apart, from the window's top left, where the program clears the window's state:
/// `mov [r+s], 5` and then `[r+s+0x20]` the window's top, `[+0x2c]`, `[+0x30]` the text and
/// `[+0x34]`, `[+0x38]` the name.
pub fn message_window(exe: &[u8]) -> Option<Window> {
    let pe = Pe::parse(exe)?;
    for (start, end) in pe.code() {
        for at in start..end.saturating_sub(5) {
            if exe[at] != 0x68 {
                continue;
            }
            let Some(r) = u32_at(exe, at + 1).and_then(|v| pe.rect(v)) else {
                continue;
            };
            if r[0] != 0 || r[1] != 0 || !(640..=2048).contains(&r[2]) || !(80..300).contains(&r[3])
            {
                continue;
            }
            let v = u32_at(exe, at + 1)?;
            let part = |src: u32| -> Option<Part> {
                let lo = at.saturating_sub(0x40).max(start);
                let hi = (at + 0x400).min(end.saturating_sub(5));
                let p = (lo..hi).find(|&p| exe[p] == 0x68 && u32_at(exe, p + 1) == Some(src))?;
                let call = (p + 5..(p + 0x100).min(end.saturating_sub(5)))
                    .find(|&k| exe[k] == 0xe8 && pe.is_call(k))?;
                Some(Part {
                    src: pe.rect(src)?,
                    dst: pe.track(p.saturating_sub(0x70).max(start), call)?,
                })
            };
            let (Some(window), Some(plate)) = (part(v), part(v + 0x40)) else {
                continue;
            };
            let Some(set) = pe.window_state(window.dst[1]) else {
                continue;
            };
            let [x, y] = [window.dst[0], window.dst[1]];
            return Some(Window {
                // the end is empty (0, 0, 0, 0) when the window is narrower than the screen
                parts: [Some(window), part(v + 0x10)]
                    .into_iter()
                    .flatten()
                    .collect(),
                plate,
                text: [x + set[0], y + set[1]],
                name: [x + set[2], y + set[3]],
            });
        }
    }
    None
}

impl Pe<'_> {
    fn i32_at_va(&self, va: u32) -> Option<i32> {
        u32_at(self.b, self.offset(va)?).map(|v| v as i32)
    }

    /// Four i32 at `va` that make a rect (x0 < x1, y0 < y1).
    fn rect(&self, va: u32) -> Option<[i32; 4]> {
        let o = self.offset(va)?;
        let mut r = [0; 4];
        for (k, v) in r.iter_mut().enumerate() {
            *v = u32_at(self.b, o + 4 * k)? as i32;
        }
        let fine = 0 <= r[0] && r[0] < r[2] && r[2] <= 4096 && 0 <= r[1] && r[1] < r[3];
        (fine && r[3] <= 4096).then_some(r)
    }

    /// Whether the e8 at `k` calls into code.
    fn is_call(&self, k: usize) -> bool {
        let (Some(here), Some(rel)) = (self.va(k), u32_at(self.b, k + 1)) else {
            return false;
        };
        let to = here.wrapping_add(5).wrapping_add(rel);
        self.sections
            .iter()
            .any(|&(v, _, size, x)| x && to >= v && ((to - v) as usize) < size)
    }

    /// The rect a draw call is given, followed through the code in [lo, hi): the registers
    /// loaded from globals and added, and the stores to the frame; the rect is at the last
    /// `lea r32, [ebp+d]`.
    fn track(&self, lo: usize, hi: usize) -> Option<[i32; 4]> {
        let b = self.b;
        let mut reg: [Option<i32>; 8] = [None; 8];
        let mut mem: Vec<(i8, Option<i32>)> = Vec::new();
        let mut lea = None;
        let mut k = lo;
        while k < hi {
            let op = b[k];
            if op == 0xa1 {
                reg[0] = u32_at(b, k + 1).and_then(|g| self.i32_at_va(g));
                k += 5;
                continue;
            }
            if matches!(op, 0x8b | 0x03 | 0x2b | 0x89 | 0x8d) && k + 2 < hi {
                let m = b[k + 1];
                let (md, r, rm) = (m >> 6, ((m >> 3) & 7) as usize, (m & 7) as usize);
                if md == 0 && rm == 5 && matches!(op, 0x8b | 0x03 | 0x2b) {
                    let g = u32_at(b, k + 2).and_then(|g| self.i32_at_va(g));
                    reg[r] = match (op, reg[r], g) {
                        (0x8b, _, g) => g,
                        (0x03, Some(a), Some(g)) => Some(a.wrapping_add(g)),
                        (0x2b, Some(a), Some(g)) => Some(a.wrapping_sub(g)),
                        _ => None,
                    };
                    k += 6;
                    continue;
                }
                if md == 3 && op == 0x03 {
                    reg[r] = reg[r].zip(reg[rm]).map(|(a, c)| a.wrapping_add(c));
                    k += 2;
                    continue;
                }
                if md == 1 && rm == 5 && op == 0x89 {
                    mem.push((b[k + 2] as i8, reg[r]));
                    k += 3;
                    continue;
                }
                if md == 1 && rm == 5 && op == 0x8d {
                    lea = Some(b[k + 2] as i8);
                    k += 3;
                    continue;
                }
            }
            k += 1;
        }
        let lea = lea?;
        let slot = |d: i8| mem.iter().rev().find(|(m, _)| *m == d).and_then(|m| m.1);
        let r = [
            slot(lea)?,
            slot(lea.checked_add(4)?)?,
            slot(lea.checked_add(8)?)?,
            slot(lea.checked_add(12)?)?,
        ];
        (r[0] < r[2] && r[1] < r[3]).then_some(r)
    }

    /// Where the program clears the window's state (the window's top is `top`): the text's
    /// x and y and the name's middle and top, from the window's top left.
    fn window_state(&self, top: i32) -> Option<[i32; 4]> {
        let b = self.b;
        for (start, end) in self.code() {
            for k in start..end.saturating_sub(10) {
                // mov dword [r32 + disp32], 5 (not esp)
                if b[k] != 0xc7 || !(0x80..=0x87).contains(&b[k + 1]) || b[k + 1] == 0x84 {
                    continue;
                }
                let (m, Some(disp)) = (b[k + 1], u32_at(b, k + 2)) else {
                    continue;
                };
                if u32_at(b, k + 6) != Some(5) {
                    continue;
                }
                let lo = k.saturating_sub(0x40).max(start);
                let hi = (k + 0x80).min(end.saturating_sub(10));
                let store = |off: u32| {
                    (lo..hi)
                        .find(|&j| {
                            b[j] == 0xc7
                                && b[j + 1] == m
                                && u32_at(b, j + 2) == Some(disp.wrapping_add(off))
                        })
                        .and_then(|j| u32_at(b, j + 6))
                        .map(|v| v as i32)
                };
                if store(0x20) != Some(top) {
                    continue;
                }
                if let (Some(tx), Some(ty), Some(nx), Some(ny)) =
                    (store(0x2c), store(0x30), store(0x34), store(0x38))
                {
                    return Some([tx, ty, nx, ny]);
                }
            }
        }
        None
    }
}
