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
    #[error("Choose a Game Boy Advance Logiqx XML DAT with SHA-1 hashes")]
    Catalog,
    #[error("The folder is too large or nested too deeply; choose a smaller folder")]
    ScanLimit,
    #[error("A selected ROM changed or became unavailable; choose the folder again")]
    Changed,
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
pub struct GbaCatalog {
    pub name: String,
    entries: BTreeMap<(String, u64), BTreeSet<String>>,
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

impl GbaCatalog {
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
        if !root
            || !closed
            || !catalog
                .name
                .to_ascii_lowercase()
                .contains("game boy advance")
            || catalog.entries.is_empty()
        {
            return Err(RomImportError::Catalog);
        }
        Ok(catalog)
    }
}

/// Streams a raw GBA file with an upper bound; SHA-1 is catalog compatibility,
/// SHA-256 is Horizon's content identity. Neither is an achievement identifier.
pub fn fingerprint(path: &Path) -> Result<RomFingerprint, RomImportError> {
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
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
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
    Ok(RomFingerprint {
        sha1: hex(sha1.finish().as_ref()),
        sha256: hex(sha256.finish().as_ref()),
        size,
    })
}

pub fn scan(folder: &Path, catalog: Option<&GbaCatalog>) -> Result<RomScan, RomImportError> {
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
            if !kind.is_file()
                || !path
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("gba"))
            {
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
                title,
                fingerprint: None,
                catalog: None,
                status: String::new(),
                selected: false,
            };
            match fingerprint(&candidate.path) {
                Ok(hash) => {
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
        let catalog = GbaCatalog::parse(&xml[..]).unwrap();
        assert_eq!(catalog.entries[&("a".repeat(40), 192)].len(), 2);
        assert!(!catalog.entries.contains_key(&("a".repeat(40), 193)));
        assert!(
            GbaCatalog::parse(
                &b"<datafile><header><name>Other system</name></header></datafile>"[..]
            )
            .is_err()
        );
        assert!(GbaCatalog::parse(&b"not XML"[..]).is_err());
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
        let catalog = GbaCatalog::parse(xml.as_bytes()).unwrap();
        let result = scan(&dir, Some(&catalog)).unwrap();
        assert_eq!(result.candidates[0].title, "Actual & Title (Europe)");
        assert!(result.candidates[0].selected);
        assert!(GbaCatalog::parse(xml.trim_end_matches("</datafile>").as_bytes()).is_err());
        fs::write(dir.join("Broken.gba"), [0u8; 10]).unwrap();
        let result = scan(&dir, Some(&catalog)).unwrap();
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
}
