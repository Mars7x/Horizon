//! Uncompressed 3DS cartridge/application and homebrew recognition.
//! No keys, installation, decryption or system-file discovery happens here.
use super::gba::recognition::RomImportError;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub fn validate(file: &mut File, path: &Path, size: u64) -> Result<(), RomImportError> {
    let mut header = [0u8; 0x200];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut header[..0x20])?;
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("3dsx"))
    {
        let header_size = u16::from_le_bytes([header[4], header[5]]) as u64;
        if &header[..4] != b"3DSX" || !(0x20..=size).contains(&header_size) {
            return Err(RomImportError::Changed);
        }
    } else {
        file.read_exact(&mut header[0x20..])?;
        let word = |offset| {
            u32::from_le_bytes([
                header[offset],
                header[offset + 1],
                header[offset + 2],
                header[offset + 3],
            ]) as u64
        };
        let (offset, length) = match &header[0x100..0x104] {
            b"NCSD" => (word(0x120) * 0x200, word(0x124) * 0x200),
            b"NCCH" => (0, size),
            _ => return Err(RomImportError::Changed),
        };
        if length < 0x200 || offset.checked_add(length).is_none_or(|end| end > size) {
            return Err(RomImportError::Changed);
        }
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut header)?;
        if &header[0x100..0x104] != b"NCCH" {
            return Err(RomImportError::Changed);
        }
        if header[0x18f] & 4 == 0 {
            return Err(RomImportError::Encrypted3ds);
        }
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_decrypted_content_and_rejects_encryption_and_truncation() {
        let directory =
            std::env::temp_dir().join(format!("horizon-3ds-recognition-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("game.3ds");
        let mut bytes = vec![0; 0x400];
        bytes[0x100..0x104].copy_from_slice(b"NCSD");
        bytes[0x120..0x124].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x124..0x128].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x300..0x304].copy_from_slice(b"NCCH");
        bytes[0x38f] = 4;
        std::fs::write(&path, &bytes).unwrap();
        let valid = super::super::gba::recognition::fingerprint(&path).unwrap();
        assert_eq!(valid.size, 0x400);
        bytes[0x38f] = 0;
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            super::super::gba::recognition::fingerprint(&path),
            Err(RomImportError::Encrypted3ds)
        ));
        bytes[0x38f] = 4;
        bytes[0x124..0x128].copy_from_slice(&2u32.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert!(super::super::gba::recognition::fingerprint(&path).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn homebrew_is_identified_by_magic_without_claiming_generic_elf_files() {
        let path =
            std::env::temp_dir().join(format!("horizon-homebrew-{}.3dsx", std::process::id()));
        let mut bytes = [0u8; 0x20];
        bytes[..4].copy_from_slice(b"3DSX");
        bytes[4..6].copy_from_slice(&0x20u16.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(super::super::gba::recognition::fingerprint(&path).is_ok());
        assert!(
            super::super::gba::recognition::RomSystem::from_extension(Path::new("game.elf"))
                .is_none()
        );
        std::fs::remove_file(path).unwrap();
    }
}
