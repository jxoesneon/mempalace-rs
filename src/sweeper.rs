//! Sweeper / cleanup utilities for the MemPalace palace.
//!
//! A forward-port of the upstream `sweep` concept. This stub prints a notice
//! while the real implementation (orphaned vector cleanup, empty-wing pruning,
//! etc.) is ported upstream.

use anyhow::Result;
use std::path::Path;

/// Run a placeholder sweep of the palace.
pub fn sweep(palace_dir: impl AsRef<Path>) -> Result<()> {
    let _ = palace_dir.as_ref();
    println!("sweep: not yet implemented");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sweep_stub() {
        let dir = tempdir().unwrap();
        assert!(sweep(dir.path()).is_ok());
    }
}
