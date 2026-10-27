//! .srp scenario scripts: a list of commands.
//!
//! ```text
//! count:u32
//! count x { size:u16 group:u16 op:u16 args:[u8; size - 4] }
//! ```
//!
//! The arguments are stored with the halves of each byte swapped. Strings in them are
//! Shift_JIS and end at the end of the arguments (or at a 0 byte).
//!
//! The 2006-2008 games have an older form: count:u32, then text lines that each end with a 0
//! byte, every byte stored as `swap_halves(b) ^ 0x0a`. The first byte of a line is the command,
//! the rest its arguments (small numbers as bytes, often plus 0x14, larger ones as hex or
//! decimal digits). These become commands of group [`OLD`].

/// The group of the commands of the older form.
pub const OLD: u16 = 0xf0;

#[derive(Debug, Clone)]
pub struct Command {
    pub group: u16,
    pub op: u16,
    pub args: Vec<u8>,
}

impl Command {
    pub fn u8(&self, at: usize) -> u8 {
        self.args.get(at).copied().unwrap_or(0)
    }
    pub fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.u8(at), self.u8(at + 1)])
    }
    pub fn i16(&self, at: usize) -> i32 {
        self.u16(at) as i16 as i32
    }
    pub fn u32(&self, at: usize) -> u32 {
        self.u16(at) as u32 | (self.u16(at + 2) as u32) << 16
    }
    /// The Shift_JIS bytes from `at` to the end (or a 0 byte).
    pub fn bytes(&self, at: usize) -> &[u8] {
        let b = self.args.get(at..).unwrap_or(&[]);
        &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
    }
    pub fn str(&self, at: usize) -> String {
        nexas::sjis::decode(self.bytes(at))
    }
}

pub struct Script {
    pub name: String,
    pub commands: Vec<Command>,
}

impl Script {
    pub fn parse(name: &str, b: &[u8]) -> Result<Script, String> {
        let commands = match Script::parse_commands(name, b) {
            Ok(c) => c,
            Err(e) => Script::parse_old(b).ok_or(e)?,
        };
        Ok(Script {
            name: name.to_string(),
            commands,
        })
    }

    fn parse_commands(name: &str, b: &[u8]) -> Result<Vec<Command>, String> {
        let err = || format!("{name}: script is cut short");
        let count = u32::from_le_bytes(b.get(0..4).ok_or_else(err)?.try_into().unwrap());
        let mut at = 4;
        let mut commands = Vec::new();
        for _ in 0..count {
            let head = b.get(at..at + 6).ok_or_else(err)?;
            let size = u16::from_le_bytes([head[0], head[1]]) as usize;
            let args = b.get(at + 6..at + 2 + size.max(4)).ok_or_else(err)?;
            commands.push(Command {
                group: u16::from_le_bytes([head[2], head[3]]),
                op: u16::from_le_bytes([head[4], head[5]]),
                args: args.iter().map(|c| c.rotate_left(4)).collect(),
            });
            if commands
                .last()
                .is_some_and(|c: &Command| c.group > 0x50 || !c.group.is_multiple_of(0x10))
            {
                return Err(format!("{name}: not a script"));
            }
            at += 2 + size.max(4);
        }
        if at != b.len() {
            return Err(format!("{name}: not a script"));
        }
        Ok(commands)
    }

    fn parse_old(b: &[u8]) -> Option<Vec<Command>> {
        let count = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?) as usize;
        let text: Vec<u8> = b[4..].iter().map(|&c| (c ^ 0x0a).rotate_left(4)).collect();
        let commands: Vec<Command> = text
            .split(|&c| c == 0)
            .take(count)
            .map(|line| Command {
                group: OLD,
                op: line.first().copied().unwrap_or(0) as u16,
                args: line.get(1..).unwrap_or(&[]).to_vec(),
            })
            .collect();
        (commands.len() == count).then_some(commands)
    }

    /// The command after the label `name`.
    pub fn label(&self, name: &str) -> Option<usize> {
        self.commands
            .iter()
            .position(|c| {
                matches!((c.group, c.op), (0x10, 0) | (OLD, 0x21))
                    && c.str(0).eq_ignore_ascii_case(name)
            })
            .map(|i| i + 1)
    }
}
