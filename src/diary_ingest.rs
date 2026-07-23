//! Diary ingestion utilities for the MemPalace palace.
//!
//! A forward-port of the upstream `diary_ingest` concept. This module will
//! eventually import daily/weekly diary files into the palace; for now it
//! exposes a stable stub.

use anyhow::Result;
use std::path::Path;

/// Run a placeholder diary ingestion for the given file or directory.
pub fn ingest_diary(path: impl AsRef<Path>) -> Result<()> {
    let _ = path.as_ref();
    println!("diary-ingest: not yet implemented");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ingest_diary_stub() {
        let dir = tempdir().unwrap();
        assert!(ingest_diary(dir.path()).is_ok());
    }
}
