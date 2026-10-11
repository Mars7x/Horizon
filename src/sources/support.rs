use std::{
    collections::BTreeSet,
    env,
    ffi::CString,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// Whether this process may write in `folder`. Asks the kernel, so a
/// read-only sandbox mount answers no even though the folder's permission
/// bits say yes.
pub(super) fn writable(folder: &Path) -> bool {
    CString::new(folder.as_os_str().as_bytes())
        // SAFETY: a valid NUL-terminated path for the call's duration.
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
}

pub(super) fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

pub(super) fn host_config_home() -> Option<PathBuf> {
    if env::var_os("FLATPAK_ID").is_some() {
        env::var_os("HOST_XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(".config")))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(".config")))
    }
}

pub(super) fn host_state_home() -> Option<PathBuf> {
    if env::var_os("FLATPAK_ID").is_some() {
        env::var_os("HOST_XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(".local/state")))
    } else {
        env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(".local/state")))
    }
}

pub(super) fn dedupe_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

pub(super) fn percent_encode_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_component_encoding_is_utf8_and_path_safe() {
        assert_eq!(percent_encode_component("My Game/β"), "My%20Game%2F%CE%B2");
        assert_eq!(percent_encode_component("abc-_.~123"), "abc-_.~123");
    }
}
