//! The agent's result buffer (decision 0011): sealed batches wait in memory and spill to an
//! append-only segment log in `buffer/` only while the server is unreachable or delivery falls
//! behind, so a healthy agent writes nothing to disk.
//!
//! Each record is `len u32 | crc32 u32 | state u8 | results u32 | id_len u8 | id | body`, little
//! endian, where `body` is the batch's results as JSON and the checksum covers everything after
//! `state`. Every spilled record is synced before the spill returns. A torn record at a
//! segment's end fails its length or checksum and is cut off when the buffer opens. A delivered
//! record gets `state = 1` in place, unsynced: after a power loss it may be sent again, and the
//! server answers it as a repeat. A segment is deleted once every record in it is delivered.
//!
//! Disk use stays under the configured size by deleting the oldest segment; its undelivered
//! results count as dropped, kept in the `dropped` file and reported with every poll.
//!
//! All file work is synchronous on the caller's thread. It happens only while results spill or
//! drain, at most one synced write per second.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use v1per_probe_core::wire::ProbeResult;

const DIR: &str = "buffer";
const DROPPED_FILE: &str = "dropped";
const SEGMENT_EXT: &str = "seg";
/// `len`, `crc32`, `state`, `results` and `id_len`.
const HEADER: usize = 14;
const DELIVERED: u8 = 1;
/// Batches held in memory before they spill even while delivery works: about ten seconds of
/// results.
const SPILL_AFTER: usize = 10;

/// A sealed batch. Its ID names the same results on every resend.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub id: String,
    pub results: Vec<ProbeResult>,
}

pub struct Buffer(Mutex<State>);

struct State {
    dir: PathBuf,
    max_bytes: u64,
    segment_bytes: u64,
    /// Random per process, so batch IDs never repeat those of an earlier run.
    run: String,
    sealed: u64,
    memory: VecDeque<Batch>,
    /// Oldest first; every batch on disk is older than every batch in memory.
    segments: VecDeque<Segment>,
    /// The last delivery or poll failed: new batches go straight to disk.
    offline: bool,
    disk_bytes: u64,
    disk_results: u64,
    dropped: u64,
}

struct Segment {
    number: u64,
    bytes: u64,
    /// Offset of the oldest undelivered record.
    next: u64,
    pending: u64,
}

struct Header {
    len: usize,
    crc: u32,
    state: u8,
    results: u32,
    id_len: usize,
}

impl Header {
    fn parse(bytes: &[u8; HEADER]) -> Self {
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
        Self {
            len: u32_at(0) as usize,
            crc: u32_at(4),
            state: bytes[8],
            results: u32_at(9),
            id_len: bytes[13] as usize,
        }
    }

    /// The whole record's size on disk.
    fn size(&self) -> u64 {
        (HEADER + self.id_len + self.len) as u64
    }
}

impl Buffer {
    /// Opens the log in `data_dir`, cutting off torn records. `max_bytes` bounds its disk use.
    pub fn open(data_dir: &Path, max_bytes: u64) -> io::Result<Self> {
        let dir = data_dir.join(DIR);
        fs::create_dir_all(&dir)?;
        let mut numbers = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == SEGMENT_EXT)
                && let Some(number) = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| stem.parse::<u64>().ok())
            {
                numbers.push(number);
            }
        }
        numbers.sort_unstable();
        let mut segments = VecDeque::new();
        for number in numbers {
            let path = segment_path(&dir, number);
            let segment = recover(&path, number)?;
            if segment.pending == 0 {
                fs::remove_file(&path)?;
            } else {
                segments.push_back(segment);
            }
        }
        let dropped = match fs::read_to_string(dir.join(DROPPED_FILE)) {
            Ok(text) => text.trim().parse().unwrap_or(0),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        let mut run = [0u8; 8];
        getrandom::fill(&mut run).map_err(io::Error::other)?;
        Ok(Self(Mutex::new(State {
            dir,
            max_bytes,
            // A dropped segment takes at most an eighth of the buffer with it.
            segment_bytes: (max_bytes / 8).clamp(64 * 1024, 4 * 1024 * 1024),
            run: run.iter().map(|byte| format!("{byte:02x}")).collect(),
            sealed: 0,
            memory: VecDeque::new(),
            disk_bytes: segments.iter().map(|s| s.bytes).sum(),
            disk_results: segments.iter().map(|s| s.pending).sum(),
            segments,
            offline: false,
            dropped,
        })))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().expect("buffer state")
    }

    /// Results waiting for delivery, in memory and on disk.
    pub fn buffered(&self) -> u64 {
        let state = self.state();
        state.disk_results
            + state
                .memory
                .iter()
                .map(|b| b.results.len() as u64)
                .sum::<u64>()
    }

    /// Results dropped for space since the buffer was created.
    pub fn dropped(&self) -> u64 {
        self.state().dropped
    }

    /// Seals `results` as one batch. It spills while the server is unreachable or when too
    /// many batches wait.
    pub fn push(&self, results: Vec<ProbeResult>) -> io::Result<()> {
        let mut state = self.state();
        state.sealed += 1;
        let id = format!("{}-{}", state.run, state.sealed);
        state.memory.push_back(Batch { id, results });
        if state.offline || state.memory.len() > SPILL_AFTER {
            state.spill()?;
        }
        Ok(())
    }

    /// Records whether the server answered. Going offline spills what waits in memory.
    pub fn set_offline(&self, offline: bool) -> io::Result<()> {
        let mut state = self.state();
        state.offline = offline;
        if offline { state.spill() } else { Ok(()) }
    }

    /// Writes what waits in memory to disk, as at shutdown.
    pub fn spill(&self) -> io::Result<()> {
        self.state().spill()
    }

    /// The oldest batch waiting for delivery.
    pub fn oldest(&self) -> io::Result<Option<Batch>> {
        let state = self.state();
        match state.segments.front() {
            Some(segment) => state.read(segment).map(Some),
            None => Ok(state.memory.front().cloned()),
        }
    }

    /// Forgets a delivered batch. One dropped for space meanwhile is already gone.
    pub fn delivered(&self, batch: &Batch) -> io::Result<()> {
        let mut guard = self.state();
        let state = &mut *guard;
        if let Some(segment) = state.segments.front() {
            let path = segment_path(&state.dir, segment.number);
            let file = OpenOptions::new().read(true).write(true).open(&path)?;
            let (header, id) = read_head(&file, segment.next)?;
            if id != batch.id {
                return Ok(());
            }
            file.write_all_at(&[DELIVERED], segment.next + 8)?;
            let segment = state.segments.front_mut().expect("checked above");
            segment.next += header.size();
            segment.pending -= u64::from(header.results);
            state.disk_results -= u64::from(header.results);
            let done = segment.pending == 0;
            let (number, bytes) = (segment.number, segment.bytes);
            if done {
                fs::remove_file(path)?;
                state.segments.pop_front();
                state.disk_bytes -= bytes;
                tracing::debug!(segment = number, "result segment drained");
            }
        } else if state
            .memory
            .front()
            .is_some_and(|front| front.id == batch.id)
        {
            state.memory.pop_front();
        }
        Ok(())
    }
}

impl State {
    fn spill(&mut self) -> io::Result<()> {
        if self.memory.is_empty() {
            return Ok(());
        }
        let mut records = Vec::new();
        let mut appended = Vec::with_capacity(self.memory.len());
        for batch in &self.memory {
            let start = records.len();
            encode(batch, &mut records);
            appended.push((records.len() - start, batch.results.len() as u64));
        }
        let size: u64 = records.len() as u64;
        let roll = match self.segments.back() {
            Some(tail) => tail.bytes > 0 && tail.bytes + size > self.segment_bytes,
            None => true,
        };
        if roll {
            let number = self.segments.back().map_or(0, |tail| tail.number + 1);
            File::create_new(segment_path(&self.dir, number))?;
            File::open(&self.dir)?.sync_all()?;
            self.segments.push_back(Segment {
                number,
                bytes: 0,
                next: 0,
                pending: 0,
            });
        }
        let tail = self.segments.back_mut().expect("a tail segment exists");
        let mut file = OpenOptions::new()
            .append(true)
            .open(segment_path(&self.dir, tail.number))?;
        file.write_all(&records)?;
        file.sync_data()?;
        let results: u64 = appended.iter().map(|(_, results)| results).sum();
        tail.bytes += size;
        tail.pending += results;
        self.disk_bytes += size;
        self.disk_results += results;
        self.memory.clear();
        self.trim()
    }

    /// Deletes the oldest segments while the log is over its size. The newest always stays.
    fn trim(&mut self) -> io::Result<()> {
        let mut dropped = 0;
        while self.disk_bytes > self.max_bytes && self.segments.len() > 1 {
            let oldest = self.segments.pop_front().expect("more than one segment");
            fs::remove_file(segment_path(&self.dir, oldest.number))?;
            self.disk_bytes -= oldest.bytes;
            self.disk_results -= oldest.pending;
            dropped += oldest.pending;
        }
        if dropped > 0 {
            self.dropped += dropped;
            let temporary = self.dir.join(format!("{DROPPED_FILE}.tmp"));
            fs::write(&temporary, self.dropped.to_string())?;
            fs::rename(temporary, self.dir.join(DROPPED_FILE))?;
            tracing::warn!(
                dropped,
                total = self.dropped,
                "result buffer full; dropped the oldest results"
            );
        }
        Ok(())
    }

    fn read(&self, segment: &Segment) -> io::Result<Batch> {
        let mut file = File::open(segment_path(&self.dir, segment.number))?;
        let (header, id) = read_head(&file, segment.next)?;
        let mut body = vec![0; header.len];
        file.seek(SeekFrom::Start(
            segment.next + (HEADER + header.id_len) as u64,
        ))?;
        file.read_exact(&mut body)?;
        Ok(Batch {
            id,
            results: serde_json::from_slice(&body).map_err(io::Error::other)?,
        })
    }
}

fn segment_path(dir: &Path, number: u64) -> PathBuf {
    dir.join(format!("{number:016}.{SEGMENT_EXT}"))
}

fn encode(batch: &Batch, out: &mut Vec<u8>) {
    let body = serde_json::to_vec(&batch.results).expect("results serialize");
    let id = batch.id.as_bytes();
    let id_len = u8::try_from(id.len()).expect("batch IDs are short");
    let mut checked = Vec::with_capacity(5 + id.len() + body.len());
    checked.extend_from_slice(&(batch.results.len() as u32).to_le_bytes());
    checked.push(id_len);
    checked.extend_from_slice(id);
    checked.extend_from_slice(&body);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&checked).to_le_bytes());
    out.push(0);
    out.extend_from_slice(&checked);
}

fn read_head(file: &File, offset: u64) -> io::Result<(Header, String)> {
    let mut bytes = [0u8; HEADER];
    file.read_exact_at(&mut bytes, offset)?;
    let header = Header::parse(&bytes);
    let mut id = vec![0; header.id_len];
    file.read_exact_at(&mut id, offset + HEADER as u64)?;
    let id = String::from_utf8(id).map_err(io::Error::other)?;
    Ok((header, id))
}

/// Scans a segment: where its undelivered records start and how many results they hold. A
/// record that is cut short or fails its checksum, and everything after it, is truncated.
fn recover(path: &Path, number: u64) -> io::Result<Segment> {
    let data = fs::read(path)?;
    let mut segment = Segment {
        number,
        bytes: 0,
        next: 0,
        pending: 0,
    };
    let mut at = 0;
    let mut found_pending = false;
    while at + HEADER <= data.len() {
        let header = Header::parse(data[at..at + HEADER].try_into().expect("header size"));
        let end = at + header.size() as usize;
        if end > data.len() || crc32fast::hash(&data[at + 9..end]) != header.crc {
            break;
        }
        if header.state != DELIVERED {
            if !found_pending {
                segment.next = at as u64;
                found_pending = true;
            }
            segment.pending += u64::from(header.results);
        }
        at = end;
    }
    if at < data.len() {
        tracing::warn!(segment = %path.display(), cut = data.len() - at, "cut off a torn record");
        OpenOptions::new()
            .write(true)
            .open(path)?
            .set_len(at as u64)?;
    }
    segment.bytes = at as u64;
    if !found_pending {
        segment.next = segment.bytes;
    }
    Ok(segment)
}

#[cfg(test)]
mod tests {
    use v1per_protocol::ids::MonitorId;
    use v1per_protocol::operations::monitors::HeartbeatStatus;

    use super::*;

    fn result(time: i64) -> ProbeResult {
        ProbeResult {
            monitor: MonitorId::new(1),
            revision: 1,
            time,
            status: HeartbeatStatus::Up,
            retries: 0,
            latency_ms: Some(5),
            message: "200 - OK".into(),
            http_status: Some(200),
            cert_expiry: None,
            tls: None,
        }
    }

    fn times(batch: &Batch) -> Vec<i64> {
        batch.results.iter().map(|r| r.time).collect()
    }

    fn segments(dir: &Path) -> usize {
        fs::read_dir(dir.join(DIR))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "seg")
            })
            .count()
    }

    /// A healthy agent keeps batches in memory and never touches disk.
    #[test]
    fn delivers_from_memory_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let buffer = Buffer::open(dir.path(), 1 << 20).unwrap();
        buffer.push(vec![result(0)]).unwrap();
        buffer.push(vec![result(1)]).unwrap();
        let oldest = buffer.oldest().unwrap().unwrap();
        assert_eq!(times(&oldest), [0]);
        buffer.delivered(&oldest).unwrap();
        assert_eq!(times(&buffer.oldest().unwrap().unwrap()), [1]);
        assert_eq!(segments(dir.path()), 0);
    }

    /// Offline, batches spill to disk and survive a restart in order, with the same IDs, and
    /// delivered ones stay delivered.
    #[test]
    fn spills_while_offline_and_drains_in_order_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let buffer = Buffer::open(dir.path(), 1 << 20).unwrap();
        buffer.push(vec![result(0)]).unwrap();
        buffer.set_offline(true).unwrap();
        for time in 1..4 {
            buffer.push(vec![result(time)]).unwrap();
        }
        let first = buffer.oldest().unwrap().unwrap();
        buffer.delivered(&first).unwrap();
        let second = buffer.oldest().unwrap().unwrap();
        drop(buffer);

        let reopened = Buffer::open(dir.path(), 1 << 20).unwrap();
        assert_eq!(reopened.buffered(), 3);
        let again = reopened.oldest().unwrap().unwrap();
        assert_eq!(again, second);
        reopened.delivered(&again).unwrap();
        reopened.push(vec![result(9)]).unwrap();
        let mut rest = Vec::new();
        while let Some(batch) = reopened.oldest().unwrap() {
            rest.extend(times(&batch));
            reopened.delivered(&batch).unwrap();
        }
        assert_eq!(rest, [2, 3, 9]);
        assert_eq!((reopened.buffered(), segments(dir.path())), (0, 0));
    }

    /// Over its size the log drops whole old segments, counts them across a restart and keeps
    /// the newest results.
    #[test]
    fn drops_the_oldest_segments_when_full() {
        let dir = tempfile::tempdir().unwrap();
        let max = 256 * 1024;
        let buffer = Buffer::open(dir.path(), max).unwrap();
        buffer.set_offline(true).unwrap();
        let batch: Vec<ProbeResult> = (0..100).map(result).collect();
        for _ in 0..100 {
            buffer.push(batch.clone()).unwrap();
        }
        let dropped = buffer.dropped();
        assert!(dropped > 0);
        assert_eq!(buffer.buffered() + dropped, 100 * 100);
        let on_disk: u64 = fs::read_dir(dir.path().join(DIR))
            .unwrap()
            .map(|e| e.unwrap().metadata().unwrap().len())
            .sum();
        assert!(on_disk <= max + 16, "{on_disk}");
        drop(buffer);
        assert_eq!(Buffer::open(dir.path(), max).unwrap().dropped(), dropped);
    }

    /// A record cut short by a crash is truncated; the records before it survive.
    #[test]
    fn cuts_off_a_torn_record() {
        let dir = tempfile::tempdir().unwrap();
        let buffer = Buffer::open(dir.path(), 1 << 20).unwrap();
        buffer.set_offline(true).unwrap();
        buffer.push(vec![result(0)]).unwrap();
        buffer.push(vec![result(1)]).unwrap();
        drop(buffer);
        let path = segment_path(&dir.path().join(DIR), 0);
        let len = fs::metadata(&path).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(len - 3)
            .unwrap();

        let reopened = Buffer::open(dir.path(), 1 << 20).unwrap();
        assert_eq!(reopened.buffered(), 1);
        let only = reopened.oldest().unwrap().unwrap();
        assert_eq!(times(&only), [0]);
        reopened.delivered(&only).unwrap();
        assert_eq!(reopened.oldest().unwrap(), None);
    }
}
