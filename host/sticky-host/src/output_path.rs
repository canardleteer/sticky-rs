//! Operator `--output` / `--report` paths vs `developer-data/`.

use std::path::{Component, Path, PathBuf};

use crate::original::Layout;

/// Absolute path plus whether it sits under this layout's `developer-data/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorOutput {
    /// Path resolved against the process cwd when it was relative.
    pub absolute: PathBuf,
    /// True when [`Self::absolute`] is under [`Layout::developer_data_root`].
    pub under_developer_data: bool,
}

/// Resolve `path` against the cwd (the file need not exist).
#[must_use]
pub fn resolve_operator_output(path: &Path) -> PathBuf {
    if path.is_absolute() {
        lexical_normalize(path)
    } else {
        match std::env::current_dir() {
            Ok(cwd) => lexical_normalize(&cwd.join(path)),
            Err(_) => lexical_normalize(path),
        }
    }
}

/// Where an operator capture would land, and whether that is gitignored.
#[must_use]
pub fn describe_operator_output(layout: &Layout, path: &Path) -> OperatorOutput {
    let absolute = resolve_operator_output(path);
    let root = resolve_operator_output(&layout.developer_data_root);
    OperatorOutput {
        under_developer_data: path_is_under(&absolute, &root),
        absolute,
    }
}

/// Print the absolute path; warn when it is outside `developer-data/`.
pub fn warn_operator_output(layout: &Layout, path: &Path) {
    let info = describe_operator_output(layout, path);
    eprintln!("writing {}", info.absolute.display());
    if !info.under_developer_data {
        eprintln!(
            "warning: that path is outside developer-data/; do not commit a UART capture or learn-uart YAML"
        );
    }
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::original::Layout;

    #[test]
    fn relative_path_under_layout_root_is_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        let path = tmp.path().join("uart-inspection-records/x.yaml");
        let info = describe_operator_output(&layout, &path);
        assert!(info.under_developer_data);
        assert!(info.absolute.is_absolute());
    }

    #[test]
    fn sibling_of_developer_data_is_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path().join("developer-data"));
        let info = describe_operator_output(&layout, Path::new("/tmp/idle-embassy.log"));
        assert!(!info.under_developer_data);
        assert_eq!(info.absolute, PathBuf::from("/tmp/idle-embassy.log"));
    }

    #[test]
    fn parent_dir_escape_is_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("developer-data");
        std::fs::create_dir_all(&root).unwrap();
        let layout = Layout::from_developer_data_root(&root);
        let escaped = root.join("../outside.log");
        let info = describe_operator_output(&layout, &escaped);
        assert!(!info.under_developer_data);
    }

    #[test]
    fn repo_root_relative_name_is_outside() {
        let layout = Layout::from_repo_root("/repo");
        let info = describe_operator_output(&layout, Path::new("/repo/idle-embassy.log"));
        assert!(!info.under_developer_data);
        let inside = describe_operator_output(
            &layout,
            Path::new("/repo/developer-data/uart-inspection-records/x.yaml"),
        );
        assert!(inside.under_developer_data);
    }
}
