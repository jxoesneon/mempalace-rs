//! Migration utilities for the MemPalace palace format.
//!
//! This is a minimal forward-port of the upstream migration surface. The full
//! migration logic is not yet implemented; this module provides stable stubs so
//! the CLI can wire the commands without blocking the build.

use anyhow::Result;
use std::path::Path;

/// Run a placeholder migration for the current palace.
pub fn migrate(palace_dir: impl AsRef<Path>) -> Result<()> {
    let _ = palace_dir.as_ref();
    println!("migrate: not yet implemented");
    Ok(())
}

/// Run a placeholder wing-specific migration.
pub fn migrate_wings(palace_dir: impl AsRef<Path>) -> Result<()> {
    let _ = palace_dir.as_ref();
    println!("migrate-wings: not yet implemented");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_migrate_stub() {
        let dir = tempdir().unwrap();
        assert!(migrate(dir.path()).is_ok());
    }

    #[test]
    fn test_migrate_wings_stub() {
        let dir = tempdir().unwrap();
        assert!(migrate_wings(dir.path()).is_ok());
    }
}
