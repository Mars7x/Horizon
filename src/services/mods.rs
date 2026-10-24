//! Per-game mods. The user puts them in the game's `mods` folder; Horizon
//! lists them, remembers which are on, and applies the enabled ones at
//! launch without changing the game file:
//!
//! - Game Boy, Game Boy Color, GBA and DS: `.ips`, `.bps` and `.ups` patches,
//!   applied in name order to the cartridge bytes in memory.
//! - 3DS: folders in Azahar's LayeredFS layout (`romfs/`, `exefs/`,
//!   `code.ips`, `exheader.bin`…), merged in name order (later ones win) into
//!   Azahar's mod folder for the title, as links to the files.
//!
//! Every call reads or writes the filesystem: workers only.
use crate::platform::data_paths;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

/// What a mod is, from what's in the mods folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Ips,
    Bps,
    Ups,
    /// A 3DS LayeredFS folder.
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModEntry {
    /// The file or folder name: the mod's identity.
    pub name: String,
    pub kind: ModKind,
    pub enabled: bool,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ModsState {
    enabled: BTreeSet<String>,
}

fn read_state(game: &Path) -> ModsState {
    fs::read(data_paths::game_mods_state_path(game))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_state(game: &Path, state: &ModsState) -> io::Result<()> {
    let path = data_paths::game_mods_state_path(game);
    let partial = path.with_extension("json.part");
    fs::write(
        &partial,
        serde_json::to_vec_pretty(state).map_err(io::Error::other)?,
    )?;
    fs::File::open(&partial)?.sync_all()?;
    fs::rename(&partial, &path)
}

/// The mods folder, created when missing so the user has somewhere to put
/// mods.
pub fn directory(game: &Path) -> io::Result<PathBuf> {
    let mods = data_paths::game_mods_directory(game);
    super::game_storage::ensure_directory(game, &mods)?;
    Ok(mods)
}

/// The game's mods, in the order they apply. New mods start off: a mod
/// changes the game only once the user turns it on.
pub fn list(game: &Path, three_ds: bool) -> io::Result<Vec<ModEntry>> {
    let mods = data_paths::game_mods_directory(game);
    let state = read_state(game);
    let mut entries = Vec::new();
    let read = match fs::read_dir(&mods) {
        Ok(read) => read,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(entries),
        Err(error) => return Err(error),
    };
    for entry in read {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        // Links could reach outside the mods folder: not mods.
        let kind = entry.file_type()?;
        let mod_kind = if three_ds {
            kind.is_dir().then_some(ModKind::Folder)
        } else if kind.is_file() {
            match Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("ips") => Some(ModKind::Ips),
                Some("bps") => Some(ModKind::Bps),
                Some("ups") => Some(ModKind::Ups),
                _ => None,
            }
        } else {
            None
        };
        if let Some(kind) = mod_kind {
            entries.push(ModEntry {
                enabled: state.enabled.contains(&name),
                name,
                kind,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Turn one mod on or off.
pub fn set_enabled(game: &Path, name: &str, enabled: bool) -> io::Result<()> {
    let mut state = read_state(game);
    if enabled {
        state.enabled.insert(name.to_owned());
    } else {
        state.enabled.remove(name);
    }
    write_state(game, &state)
}

/// Patched cartridge bytes and the names of the patches applied.
pub type Patched = (Vec<u8>, Vec<String>);

/// The cartridge with the game's enabled patches applied, in name order,
/// and the names applied; `None` when none are on. A patch that doesn't
/// fit the cartridge (another version, say) stops the launch with its name.
pub fn patch_cartridge(game: &Path, rom: &[u8]) -> Result<Option<Patched>, String> {
    let enabled: Vec<_> = list(game, false)
        .map_err(|error| format!("Could not read this game's mods: {error}"))?
        .into_iter()
        .filter(|entry| entry.enabled)
        .collect();
    if enabled.is_empty() {
        return Ok(None);
    }
    let mods = data_paths::game_mods_directory(game);
    let mut patched = rom.to_vec();
    let mut applied = Vec::new();
    for entry in enabled {
        let patch = fs::read(mods.join(&entry.name))
            .map_err(|error| format!("Could not read the mod “{}”: {error}", entry.name))?;
        patched = match entry.kind {
            ModKind::Ips => apply_ips(&patched, &patch),
            ModKind::Bps => apply_bps(&patched, &patch),
            ModKind::Ups => apply_ups(&patched, &patch),
            ModKind::Folder => continue,
        }
        .map_err(|error| format!("The mod “{}” couldn't be applied: {error}", entry.name))?;
        applied.push(entry.name);
    }
    Ok(Some((patched, applied)))
}

/// A 3DS cartridge's title (program) ID, which names its mod folder: from
/// the first NCCH partition of a `.3ds`/`.cci`, or the NCCH of a `.cxi`.
/// None for homebrew (`.3dsx`) and anything unrecognised.
pub fn three_ds_title(file: &mut fs::File) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut header = [0u8; 0x200];
    file.seek(SeekFrom::Start(0)).ok()?;
    file.read_exact(&mut header).ok()?;
    let ncch = match &header[0x100..0x104] {
        b"NCSD" => u64::from(u32::from_le_bytes(header[0x120..0x124].try_into().ok()?)) * 0x200,
        b"NCCH" => 0,
        _ => return None,
    };
    file.seek(SeekFrom::Start(ncch)).ok()?;
    file.read_exact(&mut header).ok()?;
    if &header[0x100..0x104] != b"NCCH" {
        return None;
    }
    let program = u64::from_le_bytes(header[0x118..0x120].try_into().ok()?);
    // Azahar applies a title's mods to its updates too (GetModId).
    const UPDATE: u64 = 0x0000_000e_0000_0000;
    Some(if program & 0x0000_00ff_0000_0000 == UPDATE {
        program & !UPDATE
    } else {
        program
    })
}

/// The marker in a mod folder Horizon assembled: it may replace it.
const MANAGED: &str = ".horizon-mods";

/// Assemble the enabled 3DS mod folders into Azahar's mod folder for
/// `title`, replacing what Horizon assembled before. A folder put there by
/// hand (without Horizon's marker) is left alone. Returns the names applied.
pub fn prepare_three_ds(game: &Path, title: u64) -> Result<Vec<String>, String> {
    let target = data_paths::three_ds_mods_target(game, title);
    let enabled: Vec<_> = list(game, true)
        .map_err(|error| format!("Could not read this game's mods: {error}"))?
        .into_iter()
        .filter(|entry| entry.enabled)
        .collect();
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.is_dir() && target.join(MANAGED).is_file() => {
            fs::remove_dir_all(&target)
                .map_err(|error| format!("Could not refresh this game's mods: {error}"))?;
        }
        Ok(_) => {
            if !enabled.is_empty() {
                tracing::warn!(
                    "a mod folder Horizon didn't make is in Azahar's folder; using it as it is"
                );
            }
            return Ok(Vec::new());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not inspect this game's mods: {error}")),
    }
    if enabled.is_empty() {
        return Ok(Vec::new());
    }
    let mods = data_paths::game_mods_directory(game);
    let link = |from: &Path, to: &Path| -> io::Result<()> {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        if to.exists() {
            fs::remove_file(to)?;
        }
        // A hard link costs nothing and can't reach outside; across
        // filesystems, a copy.
        fs::hard_link(from, to).or_else(|_| fs::copy(from, to).map(|_| ()))
    };
    let result = (|| -> io::Result<Vec<String>> {
        super::game_storage::ensure_directory(game, &target)?;
        fs::write(
            target.join(MANAGED),
            b"Assembled by Horizon from the game's mods folder.\n",
        )?;
        let mut applied = Vec::new();
        for entry in enabled {
            merge(&mods.join(&entry.name), &target, 0, &link)?;
            applied.push(entry.name);
        }
        Ok(applied)
    })();
    result.map_err(|error| format!("Could not prepare this game's mods: {error}"))
}

/// Link every regular file under `from` into `to`, keeping its path. Links
/// and other special files are skipped; later mods replace earlier files.
fn merge(
    from: &Path,
    to: &Path,
    depth: usize,
    link: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    if depth > 32 {
        return Err(io::Error::other("A mod folder is nested too deeply"));
    }
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = to.join(entry.file_name());
        if kind.is_dir() {
            merge(&entry.path(), &destination, depth + 1, link)?;
        } else if kind.is_file() {
            link(&entry.path(), &destination)?;
        }
    }
    Ok(())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// Patches up to 512 MiB of output: the largest cartridge Horizon imports.
const MAX_OUTPUT: usize = 512 * 1024 * 1024;

/// IPS: "PATCH", records of a 3-byte offset, 2-byte size and data (or, for
/// size 0, a run-length count and byte), then "EOF" and an optional size.
pub fn apply_ips(rom: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if !patch.starts_with(b"PATCH") {
        return Err("not an IPS patch".into());
    }
    let mut out = rom.to_vec();
    let mut at = 5;
    let take = |at: &mut usize, count: usize| -> Result<&[u8], String> {
        let bytes = patch
            .get(*at..*at + count)
            .ok_or("the patch is cut short")?;
        *at += count;
        Ok(bytes)
    };
    loop {
        let offset = take(&mut at, 3)?;
        if offset == b"EOF" {
            if let Ok(size) = take(&mut at, 3) {
                let size = (usize::from(size[0]) << 16)
                    | (usize::from(size[1]) << 8)
                    | usize::from(size[2]);
                out.truncate(size);
            }
            return Ok(out);
        }
        let offset =
            (usize::from(offset[0]) << 16) | (usize::from(offset[1]) << 8) | usize::from(offset[2]);
        let size = take(&mut at, 2)?;
        let size = (usize::from(size[0]) << 8) | usize::from(size[1]);
        let (count, data): (usize, Option<&[u8]>) = if size == 0 {
            let run = take(&mut at, 3)?;
            (
                (usize::from(run[0]) << 8) | usize::from(run[1]),
                Some(&patch[at - 1..at]),
            )
        } else {
            (size, Some(take(&mut at, size)?))
        };
        let end = offset + count;
        if end > MAX_OUTPUT {
            return Err("the patch makes the game too large".into());
        }
        if out.len() < end {
            out.resize(end, 0);
        }
        match data {
            Some(run) if size == 0 => out[offset..end].fill(run[0]),
            Some(bytes) => out[offset..end].copy_from_slice(bytes),
            None => {}
        }
    }
}

/// BPS and UPS variable-length numbers.
fn varint(patch: &[u8], at: &mut usize) -> Result<usize, String> {
    let mut value = 0usize;
    let mut shift = 1usize;
    loop {
        let byte = *patch.get(*at).ok_or("the patch is cut short")?;
        *at += 1;
        value = value
            .checked_add(
                usize::from(byte & 0x7f)
                    .checked_mul(shift)
                    .ok_or("bad number")?,
            )
            .ok_or("bad number")?;
        if byte & 0x80 != 0 {
            return Ok(value);
        }
        shift = shift.checked_mul(128).ok_or("bad number")?;
        value = value.checked_add(shift).ok_or("bad number")?;
    }
}

/// The three CRC32s ending a BPS or UPS patch: source, target, patch.
fn footer(patch: &[u8]) -> Result<(u32, u32), String> {
    let footer = patch
        .len()
        .checked_sub(12)
        .and_then(|start| patch.get(start..))
        .ok_or("the patch is cut short")?;
    let word =
        |i: usize| u32::from_le_bytes([footer[i], footer[i + 1], footer[i + 2], footer[i + 3]]);
    if crc32(&patch[..patch.len() - 4]) != word(8) {
        return Err("the patch file is damaged".into());
    }
    Ok((word(0), word(4)))
}

/// BPS (beat): source/target sizes, then read/copy actions, checked by CRC32.
pub fn apply_bps(rom: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if !patch.starts_with(b"BPS1") {
        return Err("not a BPS patch".into());
    }
    let (source_crc, target_crc) = footer(patch)?;
    if crc32(rom) != source_crc {
        return Err("it's for a different version of this game".into());
    }
    let end = patch.len() - 12;
    let mut at = 4;
    let source_size = varint(patch, &mut at)?;
    let target_size = varint(patch, &mut at)?;
    let metadata = varint(patch, &mut at)?;
    at = at
        .checked_add(metadata)
        .filter(|&a| a <= end)
        .ok_or("the patch is cut short")?;
    if source_size != rom.len() || target_size > MAX_OUTPUT {
        return Err("it's for a different version of this game".into());
    }
    let mut out = Vec::with_capacity(target_size);
    let (mut source_at, mut target_at) = (0isize, 0isize);
    let signed = |data: usize| -> isize {
        let magnitude = (data >> 1) as isize;
        if data & 1 != 0 { -magnitude } else { magnitude }
    };
    while at < end {
        let data = varint(patch, &mut at)?;
        let length = (data >> 2) + 1;
        if out.len() + length > target_size {
            return Err("the patch is damaged".into());
        }
        match data & 3 {
            0 => {
                let from = rom
                    .get(out.len()..out.len() + length)
                    .ok_or("the patch is damaged")?;
                out.extend_from_slice(from);
            }
            1 => {
                let from = patch
                    .get(at..at + length)
                    .filter(|_| at + length <= end)
                    .ok_or("the patch is damaged")?;
                out.extend_from_slice(from);
                at += length;
            }
            2 => {
                source_at += signed(varint(patch, &mut at)?);
                let start = usize::try_from(source_at).map_err(|_| "the patch is damaged")?;
                let from = rom
                    .get(start..start + length)
                    .ok_or("the patch is damaged")?;
                out.extend_from_slice(from);
                source_at += length as isize;
            }
            _ => {
                target_at += signed(varint(patch, &mut at)?);
                for _ in 0..length {
                    let index = usize::try_from(target_at).map_err(|_| "the patch is damaged")?;
                    let byte = *out.get(index).ok_or("the patch is damaged")?;
                    out.push(byte);
                    target_at += 1;
                }
            }
        }
    }
    if out.len() != target_size || crc32(&out) != target_crc {
        return Err("the result doesn't match the patch".into());
    }
    Ok(out)
}

/// UPS: sizes, then runs of XOR bytes at relative offsets, checked by CRC32.
pub fn apply_ups(rom: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if !patch.starts_with(b"UPS1") {
        return Err("not a UPS patch".into());
    }
    let (source_crc, target_crc) = footer(patch)?;
    let end = patch.len() - 12;
    let mut at = 4;
    let source_size = varint(patch, &mut at)?;
    let target_size = varint(patch, &mut at)?;
    // UPS works both ways: a patched game unpatches with the same file.
    let (expected, size) = if rom.len() == source_size && crc32(rom) == source_crc {
        (target_crc, target_size)
    } else if rom.len() == target_size && crc32(rom) == target_crc {
        (source_crc, source_size)
    } else {
        return Err("it's for a different version of this game".into());
    };
    if size > MAX_OUTPUT {
        return Err("the patch makes the game too large".into());
    }
    let mut out = rom.to_vec();
    out.resize(size, 0);
    let mut position = 0usize;
    while at < end {
        position = position
            .checked_add(varint(patch, &mut at)?)
            .ok_or("the patch is damaged")?;
        loop {
            let byte = *patch
                .get(at)
                .filter(|_| at < end)
                .ok_or("the patch is damaged")?;
            at += 1;
            if byte == 0 {
                position += 1;
                break;
            }
            if let Some(target) = out.get_mut(position) {
                *target ^= byte;
            }
            position += 1;
        }
    }
    if crc32(&out) != expected {
        return Err("the result doesn't match the patch".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("horizon-mods-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn ips_writes_records_runs_extends_and_truncates() {
        let rom = vec![0u8; 8];
        let mut patch = b"PATCH".to_vec();
        patch.extend_from_slice(&[0, 0, 2, 0, 2, 0xAA, 0xBB]); // two bytes at 2
        patch.extend_from_slice(&[0, 0, 6, 0, 0, 0, 4, 0xCC]); // run of 4 at 6
        patch.extend_from_slice(b"EOF");
        let out = apply_ips(&rom, &patch).unwrap();
        assert_eq!(out, [0, 0, 0xAA, 0xBB, 0, 0, 0xCC, 0xCC, 0xCC, 0xCC]);
        let mut truncated = patch.clone();
        truncated.extend_from_slice(&[0, 0, 5]);
        assert_eq!(apply_ips(&rom, &truncated).unwrap().len(), 5);
        assert!(apply_ips(&rom, b"PATCH\0\0").is_err());
        assert!(apply_ips(&rom, b"NOPE").is_err());
    }

    fn encode(mut value: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let x = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(0x80 | x);
                return out;
            }
            out.push(x);
            value -= 1;
        }
    }

    fn finish(mut patch: Vec<u8>, source: &[u8], target: &[u8]) -> Vec<u8> {
        patch.extend_from_slice(&crc32(source).to_le_bytes());
        patch.extend_from_slice(&crc32(target).to_le_bytes());
        let crc = crc32(&patch);
        patch.extend_from_slice(&crc.to_le_bytes());
        patch
    }

    #[test]
    fn bps_reads_copies_and_checks_versions() {
        let source = b"HELLO WORLD".to_vec();
        let target = b"HELLO THERE WORLD".to_vec();
        let mut patch = b"BPS1".to_vec();
        patch.extend(encode(source.len()));
        patch.extend(encode(target.len()));
        patch.extend(encode(0));
        patch.extend(encode((6 - 1) << 2)); // SourceRead "HELLO "
        patch.extend(encode(((6 - 1) << 2) | 1)); // TargetRead "THERE "
        patch.extend_from_slice(b"THERE ");
        patch.extend(encode(((5 - 1) << 2) | 2)); // SourceCopy "WORLD" from 6
        patch.extend(encode(6 << 1));
        let patch = finish(patch, &source, &target);
        assert_eq!(apply_bps(&source, &patch).unwrap(), target);
        assert!(
            apply_bps(b"HELLO WORLX", &patch)
                .unwrap_err()
                .contains("different version")
        );
        let mut damaged = patch.clone();
        damaged[6] ^= 1;
        assert!(apply_bps(&source, &damaged).is_err());
    }

    #[test]
    fn ups_xors_both_ways() {
        let source = b"ABCDEFGH".to_vec();
        let target = b"ABxDEFGHIJ".to_vec();
        let mut patch = b"UPS1".to_vec();
        patch.extend(encode(source.len()));
        patch.extend(encode(target.len()));
        patch.extend(encode(2)); // skip "AB"
        patch.push(b'C' ^ b'x');
        patch.push(0);
        patch.extend(encode(4)); // to offset 8
        patch.push(b'I');
        patch.push(b'J');
        patch.push(0);
        let patch = finish(patch, &source, &target);
        assert_eq!(apply_ups(&source, &patch).unwrap(), target);
        assert_eq!(apply_ups(&target, &patch).unwrap(), source);
        assert!(apply_ups(b"ZZZZZZZZ", &patch).is_err());
    }

    #[test]
    fn mods_are_listed_off_by_default_remembered_and_applied_in_order() {
        let game = temp("cartridge");
        let mods = directory(&game).unwrap();
        let mut first = b"PATCH".to_vec();
        first.extend_from_slice(&[0, 0, 0, 0, 1, 1]);
        first.extend_from_slice(b"EOF");
        let mut second = b"PATCH".to_vec();
        second.extend_from_slice(&[0, 0, 0, 0, 1, 2]);
        second.extend_from_slice(b"EOF");
        fs::write(mods.join("a-first.ips"), &first).unwrap();
        fs::write(mods.join("b-second.IPS"), &second).unwrap();
        fs::write(mods.join("readme.txt"), b"not a mod").unwrap();
        let listed = list(&game, false).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|entry| !entry.enabled));
        assert_eq!(patch_cartridge(&game, &[0; 4]).unwrap(), None);
        set_enabled(&game, "a-first.ips", true).unwrap();
        set_enabled(&game, "b-second.IPS", true).unwrap();
        let (patched, applied) = patch_cartridge(&game, &[0; 4]).unwrap().unwrap();
        assert_eq!(patched[0], 2, "later mods apply last");
        assert_eq!(applied, ["a-first.ips", "b-second.IPS"]);
        set_enabled(&game, "b-second.IPS", false).unwrap();
        assert_eq!(patch_cartridge(&game, &[0; 4]).unwrap().unwrap().0[0], 1);
        fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn three_ds_mods_merge_into_azahar_and_leave_hand_made_folders_alone() {
        let game = temp("3ds");
        let mods = directory(&game).unwrap();
        for (name, content) in [("Base", "base"), ("Override", "override")] {
            fs::create_dir_all(mods.join(name).join("romfs/data")).unwrap();
            fs::write(mods.join(name).join("romfs/data/text.bin"), content).unwrap();
        }
        fs::write(mods.join("Base/romfs/data/only-base.bin"), "kept").unwrap();
        let title = 0x0004_0000_0016_4800;
        let target = data_paths::three_ds_mods_target(&game, title);
        assert!(prepare_three_ds(&game, title).unwrap().is_empty());
        assert!(!target.exists());
        set_enabled(&game, "Base", true).unwrap();
        set_enabled(&game, "Override", true).unwrap();
        assert_eq!(
            prepare_three_ds(&game, title).unwrap(),
            ["Base", "Override"]
        );
        assert_eq!(
            fs::read_to_string(target.join("romfs/data/text.bin")).unwrap(),
            "override"
        );
        assert_eq!(
            fs::read_to_string(target.join("romfs/data/only-base.bin")).unwrap(),
            "kept"
        );
        // The originals are untouched.
        assert_eq!(
            fs::read_to_string(mods.join("Base/romfs/data/text.bin")).unwrap(),
            "base"
        );
        // Turned off: Horizon's folder goes away.
        set_enabled(&game, "Base", false).unwrap();
        set_enabled(&game, "Override", false).unwrap();
        prepare_three_ds(&game, title).unwrap();
        assert!(!target.exists());
        // A folder put there by hand is never replaced.
        fs::create_dir_all(target.join("romfs")).unwrap();
        set_enabled(&game, "Base", true).unwrap();
        assert!(prepare_three_ds(&game, title).unwrap().is_empty());
        assert!(!target.join("romfs/data").exists());
        fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn three_ds_title_comes_from_the_first_partition() {
        let dir = temp("title");
        let path = dir.join("game.3ds");
        let mut bytes = vec![0u8; 0x600];
        bytes[0x100..0x104].copy_from_slice(b"NCSD");
        bytes[0x120..0x124].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x500..0x504].copy_from_slice(b"NCCH");
        bytes[0x518..0x520].copy_from_slice(&0x0004_0000_0016_4800u64.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        let mut file = fs::File::open(&path).unwrap();
        assert_eq!(three_ds_title(&mut file), Some(0x0004_0000_0016_4800));
        fs::write(&path, b"3DSX....").unwrap();
        assert_eq!(three_ds_title(&mut fs::File::open(&path).unwrap()), None);
        fs::remove_dir_all(dir).unwrap();
    }
}
