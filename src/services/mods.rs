//! 3DS mods. The user puts each mod's folder, in Azahar's LayeredFS layout
//! (`romfs/`, `exefs/`, `code.ips`, `exheader.bin`…), in the game's `mods`
//! folder; Horizon lists them, remembers which are on, and at launch merges
//! the enabled ones in name order (later ones win) into Azahar's mod folder
//! for the title, as links to the files. The game file never changes.
//!
//! Every call reads or writes the filesystem: workers only.
use crate::platform::data_paths;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModEntry {
    /// The folder's name: the mod's identity.
    pub name: String,
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
pub fn list(game: &Path) -> io::Result<Vec<ModEntry>> {
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
        // Files aren't mods; links could reach outside the mods folder.
        if entry.file_type()?.is_dir() {
            entries.push(ModEntry {
                enabled: state.enabled.contains(&name),
                name,
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
    let enabled: Vec<_> = list(game)
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
    fn folders_are_listed_off_by_default_and_remembered() {
        let game = temp("listing");
        let mods = directory(&game).unwrap();
        fs::create_dir_all(mods.join("B Second")).unwrap();
        fs::create_dir_all(mods.join("A First")).unwrap();
        fs::create_dir_all(mods.join(".hidden")).unwrap();
        fs::write(mods.join("readme.txt"), b"not a mod").unwrap();
        let listed = list(&game).unwrap();
        let names: Vec<_> = listed.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["A First", "B Second"]);
        assert!(listed.iter().all(|entry| !entry.enabled));
        set_enabled(&game, "B Second", true).unwrap();
        assert!(list(&game).unwrap()[1].enabled);
        set_enabled(&game, "B Second", false).unwrap();
        assert!(!list(&game).unwrap()[1].enabled);
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
