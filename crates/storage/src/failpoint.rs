//! Deterministic process-kill failpoints around each admission and transfer boundary.
//!
//! With the `failpoints` feature, `V1PER_FAILPOINT=<name>:<n>` sends SIGKILL to the process
//! on the n-th hit of `<name>`. Nothing runs after the kill: no destructors, no flush, no
//! checkpoint. Without the feature every call compiles to nothing.
//!
//! Disk-full points return an injected `StorageFull` error instead, armed in-process with
//! [`fail_disk`], so tests can show that backup and retention recover from a full volume.

/// Names of every boundary, in protocol order.
pub const BOUNDARIES: [&str; 8] = [
    "admission.before_commit",
    "admission.after_commit",
    "transfer.before_assign",
    "transfer.after_assign",
    "transfer.during_apply",
    "transfer.before_notify",
    "transfer.before_retire",
    "transfer.during_retire",
];

/// Names of the injected disk-full errors.
pub const DISK_FULL: [&str; 2] = ["backup.copy", "retention.step"];

#[cfg(feature = "failpoints")]
static DISK_ARMED: std::sync::Mutex<Vec<(&str, u64)>> = std::sync::Mutex::new(Vec::new());

/// Makes the `hit`-th next pass through disk-full point `name` fail with `StorageFull`.
#[cfg(feature = "failpoints")]
pub fn fail_disk(name: &'static str, hit: u64) {
    assert!(DISK_FULL.contains(&name), "unknown disk-full point {name}");
    let mut armed = DISK_ARMED.lock().expect("disk failpoints");
    armed.retain(|(point, _)| *point != name);
    armed.push((name, hit));
}

#[cfg(feature = "failpoints")]
pub(crate) fn disk(name: &'static str) -> std::io::Result<()> {
    let mut armed = DISK_ARMED.lock().expect("disk failpoints");
    let Some(index) = armed.iter().position(|(point, _)| *point == name) else {
        return Ok(());
    };
    armed[index].1 -= 1;
    if armed[index].1 > 0 {
        return Ok(());
    }
    armed.remove(index);
    Err(std::io::Error::new(
        std::io::ErrorKind::StorageFull,
        format!("injected at {name}: no space left on device"),
    ))
}

#[cfg(not(feature = "failpoints"))]
#[inline(always)]
pub(crate) fn disk(_name: &'static str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(feature = "failpoints")]
pub(crate) fn hit(name: &'static str) {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};

    static ARMED: OnceLock<Option<(String, u64)>> = OnceLock::new();
    static HITS: AtomicU64 = AtomicU64::new(0);

    let armed = ARMED.get_or_init(|| {
        let spec = std::env::var("V1PER_FAILPOINT").ok()?;
        let (point, n) = spec.rsplit_once(':')?;
        assert!(BOUNDARIES.contains(&point), "unknown failpoint {point}");
        Some((point.to_owned(), n.parse().expect("failpoint count")))
    });
    if let Some((point, n)) = armed
        && point == name
        && HITS.fetch_add(1, Ordering::SeqCst) + 1 == *n
    {
        // SAFETY: plain syscalls on our own pid. Delivery is asynchronous, so this thread
        // parks until the kernel ends the process.
        unsafe {
            libc::kill(libc::getpid(), libc::SIGKILL);
            loop {
                libc::pause();
            }
        }
    }
}

#[cfg(not(feature = "failpoints"))]
#[inline(always)]
pub(crate) fn hit(_name: &'static str) {}
