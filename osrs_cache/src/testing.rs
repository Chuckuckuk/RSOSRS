//! Helpers for the tests of this crate and of the ones built on it (the map editor's compile
//! tests). Not part of the API.

use std::path::{Path, PathBuf};

/// An empty scratch directory for test `tag`, under the system's temp directory (not created).
pub fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("osrs_compile_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Size and modification time of every cache file in `dir`, to show a cache was not written to.
pub fn stamp(dir: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| crate::writer::is_cache_file(&e.file_name().to_string_lossy()))
        .map(|e| {
            let m = e.metadata().unwrap();
            (e.file_name().to_string_lossy().into_owned(), m.len(), m.modified().unwrap())
        })
        .collect();
    out.sort();
    out
}
