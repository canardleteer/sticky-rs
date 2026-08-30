//! Git identity baked by `sticky-host/build.rs`.

use serde::{Deserialize, Serialize};

/// Hash and dirty flag from the build that produced this binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRef {
    /// `git rev-parse HEAD`, or `unknown`.
    pub hash: String,
    /// Working tree had uncommitted changes at compile time.
    pub dirty: bool,
}

/// Host-package identity. Compare to YAML `package_git` when reviewing a report.
#[must_use]
pub fn package_git() -> GitRef {
    GitRef {
        hash: env!("PACKAGE_GIT").into(),
        dirty: env!("PACKAGE_GIT_DIRTY") == "1",
    }
}
