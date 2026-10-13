//! A private, self-contained copy of just the cache groups a program reads, so a build can run
//! without the game installed.
//!
//! A recording run notes every `(archive, group)` the program reads ([`start_recording`],
//! [`record`]); [`write_payload`] then copies those groups' raw containers out of the real cache.
//! [`pack_exe`] appends that payload and a trailer to a copy of the program's executable. At start-up
//! [`install_from_exe`] finds the trailer in the running executable and registers the payload, and
//! [`Cache::open_default`](crate::Cache::open_default) from then on hands out a cache that reads
//! from it. The payload holds Jagex's game data: it is for private use, never committed.
//!
//! Payload layout: repeated `[archive u8][group u32 LE][length u32 LE][container bytes]`. Trailer:
//! `[payload length u64 LE][MAGIC]`.

use crate::{fmt_err, Cache, Error, Result};
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex, OnceLock};

/// Closes the executable's trailer.
pub const MAGIC: &[u8; 8] = b"OSRSBNDL";

/// Raw cache containers by `(archive, group)`.
pub struct Bundle {
    data: Vec<u8>,
    entries: HashMap<(u8, u32), (usize, usize)>,
}

impl Bundle {
    /// Reads a payload as [`write_payload`] wrote it.
    pub fn parse(data: Vec<u8>) -> Result<Bundle> {
        let mut entries = HashMap::new();
        let mut at = 0usize;
        while at < data.len() {
            if at + 9 > data.len() {
                return fmt_err("bundle ends inside an entry header");
            }
            let archive = data[at];
            let group = u32::from_le_bytes(data[at + 1..at + 5].try_into().unwrap());
            let len = u32::from_le_bytes(data[at + 5..at + 9].try_into().unwrap()) as usize;
            at += 9;
            if at + len > data.len() {
                return fmt_err("bundle ends inside an entry");
            }
            entries.insert((archive, group), (at, len));
            at += len;
        }
        Ok(Bundle { data, entries })
    }

    /// The raw container of `archive/group`, if the bundle holds it.
    pub fn get(&self, archive: u8, group: u32) -> Option<&[u8]> {
        self.entries.get(&(archive, group)).map(|&(at, len)| &self.data[at..at + len])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

static INSTALLED: OnceLock<Arc<Bundle>> = OnceLock::new();

/// The bundle registered by [`install_from_exe`] or [`install`], if any.
pub fn installed() -> Option<Arc<Bundle>> {
    INSTALLED.get().cloned()
}

/// Makes `bundle` the cache every later `Cache::open_default` reads.
pub fn install(bundle: Bundle) {
    let _ = INSTALLED.set(Arc::new(bundle));
}

/// Looks for a payload at the end of the running executable and installs it. Returns whether the
/// executable carried one.
pub fn install_from_exe() -> bool {
    let read = || -> std::io::Result<Option<Vec<u8>>> {
        let mut file = File::open(std::env::current_exe()?)?;
        let size = file.metadata()?.len();
        if size < 16 {
            return Ok(None);
        }
        let mut trailer = [0u8; 16];
        file.seek(SeekFrom::End(-16))?;
        file.read_exact(&mut trailer)?;
        if &trailer[8..] != MAGIC {
            return Ok(None);
        }
        let len = u64::from_le_bytes(trailer[..8].try_into().unwrap());
        if len + 16 > size {
            return Ok(None);
        }
        file.seek(SeekFrom::End(-16 - len as i64))?;
        let mut payload = vec![0u8; len as usize];
        file.read_exact(&mut payload)?;
        Ok(Some(payload))
    };
    match read() {
        Ok(Some(payload)) => match Bundle::parse(payload) {
            Ok(bundle) => {
                install(bundle);
                true
            }
            Err(e) => {
                eprintln!("[cache] the bundle at the end of the executable is unreadable: {e}");
                false
            }
        },
        _ => false,
    }
}

// ---- recording ----

static RECORDER: OnceLock<Mutex<BTreeSet<(u8, u32)>>> = OnceLock::new();

/// From now on, every group read through a [`Cache`] is noted.
pub fn start_recording() {
    let _ = RECORDER.set(Mutex::new(BTreeSet::new()));
}

/// Notes a read (a no-op unless recording).
pub fn record(archive: u8, group: u32) {
    if let Some(recorder) = RECORDER.get() {
        recorder.lock().unwrap().insert((archive, group));
    }
}

/// Whether [`start_recording`] was called.
pub fn is_recording() -> bool {
    RECORDER.get().is_some()
}

/// Every group noted so far.
pub fn recorded() -> BTreeSet<(u8, u32)> {
    RECORDER.get().map(|r| r.lock().unwrap().clone()).unwrap_or_default()
}

/// Copies the raw containers of `keys` out of `cache` into a payload. Groups the cache cannot
/// read are skipped (and listed in the returned count).
pub fn write_payload(cache: &Cache, keys: &BTreeSet<(u8, u32)>) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0;
    for &(archive, group) in keys {
        match cache.read_raw(archive, group) {
            Ok(raw) => {
                out.push(archive);
                out.extend_from_slice(&group.to_le_bytes());
                out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
                out.extend_from_slice(&raw);
            }
            Err(_) => skipped += 1,
        }
    }
    (out, skipped)
}

/// `exe` followed by `payload` and the trailer [`install_from_exe`] looks for.
pub fn pack_exe(exe: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(exe.len() + payload.len() + 16);
    out.extend_from_slice(exe);
    out.extend_from_slice(payload);
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(MAGIC);
    out
}

/// The error for a group the bundle does not hold.
pub(crate) fn missing(archive: u8, group: u32) -> Error {
    Error::Missing(format!("group {archive}/{group} is not in the bundled cache"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_round_trips_through_an_executable_trailer() {
        let mut payload = Vec::new();
        for (archive, group, bytes) in [(2u8, 10u32, &b"abc"[..]), (7, 123456, &b"zz"[..])] {
            payload.push(archive);
            payload.extend_from_slice(&group.to_le_bytes());
            payload.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(bytes);
        }
        let packed = pack_exe(b"MZ-pretend-exe", &payload);
        let len = u64::from_le_bytes(packed[packed.len() - 16..packed.len() - 8].try_into().unwrap()) as usize;
        let tail = &packed[packed.len() - 16 - len..packed.len() - 16];
        let bundle = Bundle::parse(tail.to_vec()).unwrap();
        assert_eq!(bundle.len(), 2);
        assert_eq!(bundle.get(2, 10), Some(&b"abc"[..]));
        assert_eq!(bundle.get(7, 123456), Some(&b"zz"[..]));
        assert_eq!(bundle.get(7, 1), None);
        assert!(Bundle::parse(vec![1, 2, 3]).is_err());
    }
}
