//! One agent at a time uses a data directory. The file name matches the server's, so an old
//! `v1per probe` and `v1per-probe` cannot share one either.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

const LOCK_FILE: &str = "v1per.lock";

/// Held until dropped; the operating system releases it when the process ends.
pub struct DataLock {
    _file: File,
}

impl DataLock {
    /// Takes the lock, or `None` when another process holds it.
    pub fn try_acquire(dir: &Path) -> io::Result<Option<Self>> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(dir.join(LOCK_FILE))?;
        // SAFETY: flock on a descriptor this function owns.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Self { _file: file }));
        }
        match io::Error::last_os_error() {
            e if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            e => Err(e),
        }
    }
}
