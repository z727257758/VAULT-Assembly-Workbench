//! Machine-local locations are discovered at runtime, never embedded in defaults.
use std::path::{Path, PathBuf};
pub fn windows_font(name: &str) -> Option<PathBuf> {
    ["SystemRoot", "WINDIR"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Fonts").join(name))
        .find(|p| p.is_file())
}
pub fn directory_hint(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    path.ancestors().find(|p| p.is_dir()).map(Path::to_path_buf)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_library_picker_uses_surviving_parent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            directory_hint(&dir.path().join("moved/library")),
            Some(dir.path().to_path_buf())
        );
        assert_eq!(directory_hint(Path::new("")), None);
    }
}
