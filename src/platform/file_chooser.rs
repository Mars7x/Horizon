//! Choosing a file through the XDG FileChooser portal, the only way a
//! Flatpak app reads a file the user picks without a broad filesystem grant.
//! The portal returns a path the sandbox can read (often under the document
//! portal). Grants can persist across sessions; unavailable paths need reselection.

use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};

use ashpd::desktop::{
    ResponseError,
    file_chooser::{FileFilter, SelectedFiles},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FileChooserError {
    #[error("could not create portal runtime: {0}")]
    Runtime(#[from] std::io::Error),
    #[error("FileChooser portal request failed: {0}")]
    Portal(#[from] ashpd::Error),
    #[error("the chosen file isn't a local file")]
    NotLocal,
}

/// Ask for one image. Blocks until the dialog closes, so call it from a
/// worker thread. `Ok(None)` when the user cancels.
pub fn choose_image(title: &str) -> Result<Option<PathBuf>, FileChooserError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let response = runtime.block_on(async {
        SelectedFiles::open_file()
            .title(title)
            .modal(true)
            .multiple(false)
            .filter(
                FileFilter::new("Images")
                    .mimetype("image/png")
                    .mimetype("image/jpeg")
                    .mimetype("image/gif")
                    .mimetype("image/jxl"),
            )
            .send()
            .await?
            .response()
    });
    let files = match response {
        Ok(files) => files,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let Some(uri) = files.uris().first() else {
        return Ok(None);
    };
    file_path(uri.as_str())
        .map(Some)
        .ok_or(FileChooserError::NotLocal)
}

/// Ask for a game folder. Blocks on a worker; cancellation changes nothing.
pub fn choose_game_folder() -> Result<Option<PathBuf>, FileChooserError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let response = runtime.block_on(async {
        let request = SelectedFiles::open_file()
            .title("Choose game folder")
            .modal(true)
            .multiple(false)
            .directory(true);
        request.send().await?.response()
    });
    let files = match response {
        Ok(files) => files,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    files
        .uris()
        .first()
        .map(|uri| file_path(uri.as_str()).ok_or(FileChooserError::NotLocal))
        .transpose()
}

/// `file:///a%20b` → `/a b`. Only local files (an empty or `localhost` host).
fn file_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(OsString::from_vec(decoded)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            file_path("file:///run/user/1000/doc/ab12/My%20Picture%E2%9C%93.png"),
            Some(PathBuf::from("/run/user/1000/doc/ab12/My Picture✓.png"))
        );
        assert_eq!(
            file_path("file://localhost/tmp/a.png"),
            Some(PathBuf::from("/tmp/a.png"))
        );
        assert_eq!(file_path("https://example.com/a.png"), None);
        assert_eq!(file_path("file://server/share/a.png"), None);
        assert_eq!(file_path("file:///bad%zz"), None);
    }
}
