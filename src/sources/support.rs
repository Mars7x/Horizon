use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

/// Whether this process may write in `folder`. Asks the kernel, so a
/// read-only sandbox mount answers no even though the folder's permission
/// bits say yes.
pub(super) fn writable(folder: &Path) -> bool {
    CString::new(folder.as_os_str().as_bytes())
        // SAFETY: a valid NUL-terminated path for the call's duration.
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
}
