//! Local cartridge recognition. No ROM bytes leave the machine.
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY, SHA256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
};
use thiserror::Error;

const MAX_ROM: u64 = 32 * 1024 * 1024;
const MAX_DAT: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Error)]
pub enum RomImportError {
    #[error("Could not read the selected file or folder: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "Choose a Game Boy, Game Boy Color or Game Boy Advance Logiqx XML DAT with SHA-1 hashes"
    )]
    Catalog,
    #[error("The folder is too large or nested too deeply; choose a smaller folder")]
    ScanLimit,
    #[error("A selected ROM changed or became unavailable; choose the folder again")]
    Changed,
}

/// The cartridge systems mGBA plays. Each is its own Library source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RomSystem {
    GameBoy,
    GameBoyColor,
    GameBoyAdvance,
}

impl RomSystem {
    pub const ALL: [Self; 3] = [Self::GameBoy, Self::GameBoyColor, Self::GameBoyAdvance];

    pub fn source_id(self) -> &'static str {
        match self {
            Self::GameBoy => "gb",
            Self::GameBoyColor => "gbc",
            Self::GameBoyAdvance => "gba",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::GameBoy => "Game Boy",
            Self::GameBoyColor => "Game Boy Color",
            Self::GameBoyAdvance => "Game Boy Advance",
        }
    }

    pub fn from_source_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|system| system.source_id() == id)
    }

    /// No-Intro names every system "… Game Boy …", so test the longer names first.
    fn from_catalog_name(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        if name.contains("game boy advance") {
            Some(Self::GameBoyAdvance)
        } else if name.contains("game boy color") {
            Some(Self::GameBoyColor)
        } else if name.contains("game boy") {
            Some(Self::GameBoy)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomFingerprint {
    pub sha1: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct RomCandidate {
    pub path: PathBuf,
    pub system: RomSystem,
    pub title: String,
    pub fingerprint: Option<RomFingerprint>,
    pub catalog: Option<String>,
    pub status: String,
    pub selected: bool,
}

#[derive(Debug, Default)]
pub struct RomScan {
    pub candidates: Vec<RomCandidate>,
    pub skipped: usize,
}

#[derive(Debug)]
pub struct RomCatalog {
    pub name: String,
    pub system: RomSystem,
    entries: BTreeMap<(String, u64), BTreeSet<String>>,
}

/// One catalog per system. A user-chosen DAT replaces its system's catalog.
#[derive(Debug)]
pub struct RomCatalogs([RomCatalog; 3]);

impl RomCatalogs {
    /// Pinned, attributed metadata only. No ROM bytes or runtime download.
    pub fn bundled() -> Result<Self, RomImportError> {
        Ok(Self([
            RomCatalog::parse(include_bytes!("../../../data/catalogs/gb.xml").as_slice())?,
            RomCatalog::parse(include_bytes!("../../../data/catalogs/gbc.xml").as_slice())?,
            RomCatalog::parse(include_bytes!("../../../data/catalogs/gba.xml").as_slice())?,
        ]))
    }

    pub fn with(mut self, catalog: RomCatalog) -> Self {
        let slot = catalog.system as usize;
        self.0[slot] = catalog;
        self
    }

    pub fn get(&self, system: RomSystem) -> &RomCatalog {
        &self.0[system as usize]
    }
}

fn attribute(
    event: &BytesStart<'_>,
    key: &[u8],
    reader: &Reader<impl std::io::BufRead>,
) -> Result<Option<String>, RomImportError> {
    for attr in event.attributes() {
        let attr = attr.map_err(|_| RomImportError::Catalog)?;
        if attr.key.as_ref() == key {
            return attr
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .map(|v| Some(v.into_owned()))
                .map_err(|_| RomImportError::Catalog);
        }
    }
    Ok(None)
}

impl RomCatalog {
    /// A title is recognized only when SHA-1 and size have one catalog title.
    pub fn title_for_hash(&self, sha1: &str, size: u64) -> Option<&str> {
        let titles = self.entries.get(&(sha1.to_ascii_lowercase(), size))?;
        if titles.len() != 1 {
            return None;
        }
        titles.first().map(String::as_str)
    }

    pub fn load(path: &Path) -> Result<Self, RomImportError> {
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_DAT {
            return Err(RomImportError::Catalog);
        }
        Self::parse(BufReader::new(file.take(MAX_DAT + 1)))
    }

    fn parse(input: impl std::io::BufRead) -> Result<Self, RomImportError> {
        let mut reader = Reader::from_reader(input);
        reader.config_mut().trim_text(true);
        let mut buffer = Vec::new();
        let mut catalog = Self {
            name: String::new(),
            system: RomSystem::GameBoyAdvance,
            entries: BTreeMap::new(),
        };
        let mut title = None;
        let mut header = false;
        let mut name = false;
        let mut root = false;
        let mut closed = false;
        loop {
            match reader
                .read_event_into(&mut buffer)
                .map_err(|_| RomImportError::Catalog)?
            {
                Event::Start(ref e) if e.name().as_ref() == b"datafile" => root = true,
                Event::Start(ref e) if e.name().as_ref() == b"header" => header = true,
                Event::Start(ref e) if header && e.name().as_ref() == b"name" => name = true,
                Event::Text(ref e) if name => catalog
                    .name
                    .push_str(&e.decode().map_err(|_| RomImportError::Catalog)?),
                Event::Start(ref e)
                    if e.name().as_ref() == b"game" || e.name().as_ref() == b"machine" =>
                {
                    title = attribute(e, b"name", &reader)?;
                }
                Event::Empty(ref e) | Event::Start(ref e) if e.name().as_ref() == b"rom" => {
                    if let (Some(title), Some(hash), Some(size)) = (
                        title.as_ref(),
                        attribute(e, b"sha1", &reader)?,
                        attribute(e, b"size", &reader)?,
                    ) {
                        let size: u64 = size.parse().map_err(|_| RomImportError::Catalog)?;
                        if hash.len() != 40
                            || !hash.bytes().all(|c| c.is_ascii_hexdigit())
                            || title.trim().is_empty()
                        {
                            return Err(RomImportError::Catalog);
                        }
                        catalog
                            .entries
                            .entry((hash.to_ascii_lowercase(), size))
                            .or_default()
                            .insert(title.clone());
                    }
                }
                Event::End(ref e) => match e.name().as_ref() {
                    b"datafile" => closed = true,
                    b"header" => header = false,
                    b"name" => name = false,
                    b"game" | b"machine" => title = None,
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
        }
        let Some(system) = RomSystem::from_catalog_name(&catalog.name) else {
            return Err(RomImportError::Catalog);
        };
        if !root || !closed || catalog.entries.is_empty() {
            return Err(RomImportError::Catalog);
        }
        catalog.system = system;
        Ok(catalog)
    }
}

/// Streams a raw cartridge file with an upper bound; SHA-1 is catalog
/// compatibility, SHA-256 is Horizon's content identity. Neither is an
/// achievement identifier.
pub fn fingerprint(path: &Path) -> Result<RomFingerprint, RomImportError> {
    read_fingerprint(path).map(|(fingerprint, _)| fingerprint)
}

/// Game Boy header byte 0x143: bit 7 marks a game made for the Game Boy
/// Color (colour-only or dual-mode). The same pass reads it, so it always
/// describes the hashed bytes.
const COLOR_FLAG: usize = 0x143;

fn read_fingerprint(path: &Path) -> Result<(RomFingerprint, bool), RomImportError> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(RomImportError::Changed);
    }
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if !(192..=MAX_ROM).contains(&size) {
        return Err(RomImportError::Changed);
    }
    let mut sha1 = Context::new(&SHA1_FOR_LEGACY_USE_ONLY);
    let mut sha256 = Context::new(&SHA256);
    let mut buffer = [0u8; 65536];
    let mut read = 0u64;
    let mut color = false;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if let Some(offset) = COLOR_FLAG.checked_sub(read as usize)
            && offset < count
        {
            color = buffer[offset] & 0x80 != 0;
        }
        read += count as u64;
        if read > MAX_ROM {
            return Err(RomImportError::Changed);
        }
        sha1.update(&buffer[..count]);
        sha256.update(&buffer[..count]);
    }
    if read != size {
        return Err(RomImportError::Changed);
    }
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok((
        RomFingerprint {
            sha1: hex(sha1.finish().as_ref()),
            sha256: hex(sha256.finish().as_ref()),
            size,
        },
        color,
    ))
}

/// Game Boy and Game Boy Color share file formats. An exact hash in exactly
/// one of their catalogs decides; otherwise the cartridge header does, so
/// dual-mode games are Game Boy Color games, as No-Intro lists them.
fn classify(hash: &RomFingerprint, color: bool, catalogs: Option<&RomCatalogs>) -> RomSystem {
    let listed = |system| {
        catalogs.is_some_and(|c| {
            c.get(system)
                .entries
                .contains_key(&(hash.sha1.clone(), hash.size))
        })
    };
    match (listed(RomSystem::GameBoy), listed(RomSystem::GameBoyColor)) {
        (true, false) => RomSystem::GameBoy,
        (false, true) => RomSystem::GameBoyColor,
        _ if color => RomSystem::GameBoyColor,
        _ => RomSystem::GameBoy,
    }
}

pub fn scan(folder: &Path, catalogs: Option<&RomCatalogs>) -> Result<RomScan, RomImportError> {
    let mut scan = RomScan::default();
    let mut pending = vec![(folder.to_owned(), 0)];
    let mut visited = 0;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 16 {
            return Err(RomImportError::ScanLimit);
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            visited += 1;
            if visited > MAX_ENTRIES {
                return Err(RomImportError::ScanLimit);
            }
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push((path, depth + 1));
                continue;
            }
            let extension = path
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase);
            let advance = match extension.as_deref() {
                Some("gba") => true,
                Some("gb" | "gbc") => false,
                _ => {
                    scan.skipped += 1;
                    continue;
                }
            };
            if !kind.is_file() {
                scan.skipped += 1;
                continue;
            }
            let title = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let mut candidate = RomCandidate {
                path,
                system: if advance {
                    RomSystem::GameBoyAdvance
                } else {
                    RomSystem::GameBoy
                },
                title,
                fingerprint: None,
                catalog: None,
                status: String::new(),
                selected: false,
            };
            match read_fingerprint(&candidate.path) {
                Ok((hash, color)) => {
                    if !advance {
                        candidate.system = classify(&hash, color, catalogs);
                    }
                    let catalog = catalogs.map(|c| c.get(candidate.system));
                    let matches =
                        catalog.and_then(|c| c.entries.get(&(hash.sha1.clone(), hash.size)));
                    match matches {
                        Some(names) if names.len() == 1 => {
                            if let Some(title) = names.first() {
                                candidate.title.clone_from(title);
                            }
                            candidate.catalog = catalog.map(|c| c.name.clone());
                            candidate.status =
                                "Exact catalog match · title and region from catalog".into();
                            candidate.selected = true;
                        }
                        Some(_) => {
                            candidate.status =
                                "Ambiguous catalog match · filename only, unverified".into()
                        }
                        None => {
                            candidate.status = "No exact match · filename only, unverified".into()
                        }
                    }
                    candidate.fingerprint = Some(hash);
                }
                Err(error) => candidate.status = format!("Cannot import: {error}"),
            }
            scan.candidates.push(candidate);
        }
    }
    scan.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::rom_import::revalidate;
    #[test]
    fn catalog_matches_size_and_hash_and_retains_ambiguity() {
        let xml = br#"<datafile><header><name>Nintendo - Game Boy Advance</name></header><game name="One (USA)"><rom size="192" sha1="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"/></game><game name="Two (Europe)"><rom size="192" sha1="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"/></game></datafile>"#;
        let catalog = RomCatalog::parse(&xml[..]).unwrap();
        assert_eq!(catalog.title_for_hash(&"a".repeat(40), 192), None);
        assert_eq!(catalog.entries[&("a".repeat(40), 192)].len(), 2);
        assert!(!catalog.entries.contains_key(&("a".repeat(40), 193)));
        assert!(
            RomCatalog::parse(
                &b"<datafile><header><name>Other system</name></header></datafile>"[..]
            )
            .is_err()
        );
        assert!(RomCatalog::parse(&b"not XML"[..]).is_err());
    }
    #[test]
    fn bundled_catalog_recognizes_known_release_hashes_without_filenames() {
        let catalogs = RomCatalogs::bundled().unwrap();
        for (system, count) in [
            (RomSystem::GameBoy, 2115),
            (RomSystem::GameBoyColor, 2433),
            (RomSystem::GameBoyAdvance, 3503),
        ] {
            let catalog = catalogs.get(system);
            assert_eq!(catalog.system, system);
            assert_eq!(
                catalog.entries.values().map(BTreeSet::len).sum::<usize>(),
                count
            );
        }
        assert_eq!(
            catalogs
                .get(RomSystem::GameBoy)
                .title_for_hash("ea9bcae617fdf159b045185467ae58b2e4a48b9a", 1_048_576),
            Some("Pokemon - Red Version (USA, Europe) (SGB Enhanced)")
        );
        assert_eq!(
            catalogs
                .get(RomSystem::GameBoyColor)
                .title_for_hash("d8b8a3600a465308c9953dfa04f0081c05bdcb94", 2_097_152),
            Some("Pokemon - Gold Version (USA, Europe) (SGB Enhanced) (GB Compatible)")
        );
        let catalog = catalogs.get(RomSystem::GameBoyAdvance);
        for (sha1, expected) in [
            (
                "574fa542ffebb14be69902d1d36f1ec0a4afd71e",
                "Pokemon - LeafGreen Version (USA, Europe)",
            ),
            (
                "f3ae088181bf583e55daf962a92bb46f4f1d07b7",
                "Pokemon - Emerald Version (USA, Europe)",
            ),
        ] {
            assert_eq!(catalog.title_for_hash(sha1, 16_777_216), Some(expected));
            assert_eq!(
                catalog.title_for_hash(&sha1.to_uppercase(), 16_777_216),
                Some(expected)
            );
            assert_eq!(catalog.title_for_hash(sha1, 192), None);
        }
        assert_eq!(catalog.title_for_hash(&"0".repeat(40), 16_777_216), None);
    }
    #[test]
    fn exact_matching_uses_bytes_instead_of_filename() {
        let dir =
            std::env::temp_dir().join(format!("horizon-rom-exact-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Misleading name.GBA");
        fs::write(&path, [0u8; 192]).unwrap();
        let hash = fingerprint(&path).unwrap();
        let xml = format!(
            r#"<datafile><header><name>Nintendo - Game Boy Advance</name></header><game name="Actual &amp; Title (Europe)"><rom size="192" sha1="{}"/></game></datafile>"#,
            hash.sha1
        );
        let catalogs = RomCatalogs::bundled()
            .unwrap()
            .with(RomCatalog::parse(xml.as_bytes()).unwrap());
        let result = scan(&dir, Some(&catalogs)).unwrap();
        assert_eq!(result.candidates[0].title, "Actual & Title (Europe)");
        assert!(result.candidates[0].selected);
        assert!(RomCatalog::parse(xml.trim_end_matches("</datafile>").as_bytes()).is_err());
        fs::write(dir.join("Broken.gba"), [0u8; 10]).unwrap();
        let result = scan(&dir, Some(&catalogs)).unwrap();
        assert!(
            result
                .candidates
                .iter()
                .any(|c| c.fingerprint.is_none() && !c.selected)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scan_does_not_follow_links_and_changed_bytes_fail_review() {
        let dir = std::env::temp_dir().join(format!("horizon-rom-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Homebrew.gba");
        fs::write(&path, [0u8; 192]).unwrap();
        std::os::unix::fs::symlink(&path, dir.join("link.gba")).unwrap();
        let mut result = scan(&dir, None).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert!(!result.candidates[0].selected);
        result.candidates[0].selected = true;
        revalidate(&result.candidates).unwrap();
        fs::write(&path, [1u8; 192]).unwrap();
        assert!(revalidate(&result.candidates).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn catalog_names_choose_their_system() {
        for (name, system) in [
            ("Nintendo - Game Boy", RomSystem::GameBoy),
            ("Nintendo - Game Boy Color", RomSystem::GameBoyColor),
            ("Nintendo - Game Boy Advance", RomSystem::GameBoyAdvance),
        ] {
            let xml = format!(
                r#"<datafile><header><name>{name}</name></header><game name="One"><rom size="192" sha1="{}"/></game></datafile>"#,
                "a".repeat(40)
            );
            assert_eq!(RomCatalog::parse(xml.as_bytes()).unwrap().system, system);
        }
        for system in RomSystem::ALL {
            assert_eq!(RomSystem::from_source_id(system.source_id()), Some(system));
        }
        assert_eq!(RomSystem::from_source_id("steam"), None);
    }

    #[test]
    fn game_boy_files_follow_their_catalog_then_their_header() {
        let dir = std::env::temp_dir().join(format!("horizon-rom-gb-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cartridge = |color: u8, fill: u8| {
            let mut rom = vec![fill; 0x150];
            rom[COLOR_FLAG] = color;
            rom
        };
        fs::write(dir.join("Mono.gb"), cartridge(0x00, 1)).unwrap();
        fs::write(dir.join("Dual.gb"), cartridge(0x80, 2)).unwrap();
        fs::write(dir.join("Colour only.GBC"), cartridge(0xC0, 3)).unwrap();
        // Misnamed: the header says Game Boy, the catalog says Game Boy Color.
        fs::write(dir.join("Listed.gb"), cartridge(0x00, 4)).unwrap();
        fs::write(dir.join("Advance.gba"), [0u8; 192]).unwrap();
        fs::write(dir.join("Notes.gbx"), [0u8; 192]).unwrap();
        let listed = fingerprint(&dir.join("Listed.gb")).unwrap();
        let mut catalogs = RomCatalogs::bundled().unwrap();
        catalogs = catalogs.with(
            RomCatalog::parse(
                format!(
                    r#"<datafile><header><name>Nintendo - Game Boy Color</name></header><game name="Listed (World)"><rom size="{}" sha1="{}"/></game></datafile>"#,
                    listed.size, listed.sha1
                )
                .as_bytes(),
            )
            .unwrap(),
        );
        let result = scan(&dir, Some(&catalogs)).unwrap();
        let system = |name: &str| {
            result
                .candidates
                .iter()
                .find(|c| c.path.file_name().unwrap() == name)
                .map(|c| (c.system, c.title.as_str()))
                .unwrap()
        };
        assert_eq!(system("Mono.gb"), (RomSystem::GameBoy, "Mono"));
        assert_eq!(system("Dual.gb"), (RomSystem::GameBoyColor, "Dual"));
        assert_eq!(
            system("Colour only.GBC"),
            (RomSystem::GameBoyColor, "Colour only")
        );
        assert_eq!(
            system("Listed.gb"),
            (RomSystem::GameBoyColor, "Listed (World)")
        );
        assert_eq!(
            system("Advance.gba"),
            (RomSystem::GameBoyAdvance, "Advance")
        );
        assert_eq!((result.candidates.len(), result.skipped), (5, 1));
        fs::remove_dir_all(dir).unwrap();
    }
}
