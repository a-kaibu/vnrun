//! Synthetic PE/SDRS fixture shared by prep and server integration tests.

use std::fs;
use std::process::Output;

use sdrs_core::sdrs::crypto::decrypt_v1;
use sdrs_core::sdrs::hash::crc32;
use sdrs_core::sdrs::stub::HEADER_VERSION_V7;
use tempfile::TempDir;

pub struct Game {
    pub dir: TempDir,
    pub shell: Vec<u8>,
    pub exe: Vec<u8>,
    pub encrypted: Vec<u8>,
    pub plain: Vec<u8>,
}

impl Game {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        // Minimal PE32 with a section at file offset 0x400, ending at 0xC00.
        let mut exe = vec![0; 0xC00];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        exe[0x40..0x44].copy_from_slice(b"PE\0\0");
        exe[0x44..0x46].copy_from_slice(&0x14Cu16.to_le_bytes());
        exe[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());
        exe[0x54..0x56].copy_from_slice(&0xE0u16.to_le_bytes());
        let section = &mut exe[0x138..0x160];
        section[..8].copy_from_slice(b".text\0\0\0");
        section[8..12].copy_from_slice(&0x800u32.to_le_bytes());
        section[12..16].copy_from_slice(&0x1000u32.to_le_bytes());
        section[16..20].copy_from_slice(&0x800u32.to_le_bytes());
        section[20..24].copy_from_slice(&0x400u32.to_le_bytes());

        // A V7 shell naming game.exe, with a data.bin patch at offset zero.
        let mut args = vec![0; 0x9540];
        args[0x3C..0x40].copy_from_slice(&8u32.to_le_bytes());
        args[0x40..0x48].copy_from_slice(b"data.bin");
        args[0xC4..0xC8].copy_from_slice(&1u32.to_le_bytes());
        args[0xC8..0xCC].copy_from_slice(&16u32.to_le_bytes());
        args[0xCC..0xD0].copy_from_slice(&2u32.to_le_bytes());
        args[0x9440..0x9449].copy_from_slice(b"game.exe\0");
        let mut shell = exe.clone();
        shell.extend_from_slice(&HEADER_VERSION_V7.to_le_bytes());
        shell.extend_from_slice(&crc32(&args).to_le_bytes());
        shell.extend_from_slice(&7u32.to_le_bytes());
        shell.extend_from_slice(&0u32.to_le_bytes());
        decrypt_v1(&mut args, 7);
        shell.extend_from_slice(&args);
        shell.resize(0xC00 + 0x10000, 0);
        shell.extend_from_slice(&exe);

        let mut plain = vec![0x5A; 16];
        plain[..4].copy_from_slice(&1u32.to_le_bytes());
        let mut encrypted = plain.clone();
        encrypted[..4].copy_from_slice(&2u32.to_le_bytes());
        decrypt_v1(&mut encrypted[4..], 0);
        fs::write(dir.path().join("シェル.exe"), &shell).unwrap();
        fs::write(dir.path().join("data.bin"), &encrypted).unwrap();
        Self {
            dir,
            shell,
            exe,
            encrypted,
            plain,
        }
    }

    pub fn assert_extracted(&self, output: &Output) {
        assert_eq!(
            fs::read(self.dir.path().join("game.exe")).unwrap(),
            self.exe
        );
        assert_eq!(
            fs::read(self.dir.path().join("data.bin")).unwrap(),
            self.plain
        );
        assert!(!self.dir.path().join("シェル.exe").exists());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let backup = stdout
            .lines()
            .find_map(|line| line.strip_prefix("sdrs: originals backed up → "))
            .expect("reports the backup even when later preparation fails");
        assert_eq!(
            fs::read(format!("{backup}/シェル.exe")).unwrap(),
            self.shell
        );
        assert_eq!(
            fs::read(format!("{backup}/data.bin")).unwrap(),
            self.encrypted
        );
    }
}
