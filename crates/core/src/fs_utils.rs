use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn ensure_dir(path: &Path) -> io::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

pub fn write_string(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, content)
}

pub fn write_string_preserving_existing(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }

    if path.exists() {
        let existing = fs::read_to_string(path).unwrap_or_default();
        if existing != content {
            let backup = next_backup_path(path);
            fs::copy(path, backup)?;
        }
    }

    fs::write(path, content)
}

pub fn append_string(path: &Path, content: &str) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(content.as_bytes())
}

pub fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn relative_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(p) => normalize_path(p),
        Err(_) => normalize_path(path),
    }
}

pub fn read_to_string_lossy(path: &Path, max_bytes: u64) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > max_bytes {
        return Ok(String::new());
    }
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(_) => Ok(String::new()),
    }
}

pub fn stable_walk(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_inner(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_inner(path: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if should_ignore(path) {
        return Ok(());
    }

    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };

    if metadata.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }

    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            walk_inner(&entry, out)?;
        }
    }

    Ok(())
}

pub fn should_ignore(path: &Path) -> bool {
    let ignored = [
        ".git",
        ".hg",
        ".svn",
        "target",
        "node_modules",
        ".next",
        "dist",
        "build",
        ".cache",
        ".turbo",
        ".venv",
        "venv",
        "__pycache__",
        ".idea",
        ".DS_Store",
    ];

    path.file_name()
        .and_then(|s| s.to_str())
        .map(|name| ignored.contains(&name))
        .unwrap_or(false)
}

fn next_backup_path(path: &Path) -> PathBuf {
    for i in 1..10_000 {
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("artifact");
        let candidate = path.with_file_name(format!("{file_name}.idd-bak-{i}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path.with_extension("idd-bak")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ensure_dir_creates_missing() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().join("a/b/c");
        assert!(!target.exists());
        ensure_dir(&target).unwrap();
        assert!(target.exists());
        assert!(target.is_dir());
    }

    #[test]
    fn test_write_string_creates_parents() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().join("nested/file.txt");
        write_string(&target, "hello").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
    }

    #[test]
    fn test_write_string_preserving_existing_backups() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().join("file.txt");

        // Initial write
        write_string(&target, "v1").unwrap();

        // Same content -> no backup
        write_string_preserving_existing(&target, "v1").unwrap();
        assert!(!tmp.path().join("file.txt.idd-bak-1").exists());

        // Different content -> backup created
        write_string_preserving_existing(&target, "v2").unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("file.txt.idd-bak-1")).unwrap(),
            "v1"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "v2");

        // Second change -> second backup
        write_string_preserving_existing(&target, "v3").unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("file.txt.idd-bak-2")).unwrap(),
            "v2"
        );
    }

    #[test]
    fn test_stable_walk_is_deterministic() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        // Create files in non-alphabetical order
        fs::write(root.join("z.txt"), "").unwrap();
        fs::write(root.join("a.txt"), "").unwrap();
        fs::create_dir(root.join("subdir")).unwrap();
        fs::write(root.join("subdir/m.txt"), "").unwrap();

        let files = stable_walk(root).unwrap();
        let relative_names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();

        // subdir/ comes after a.txt but before z.txt (sorted by PathBuf components)
        // Actually walk_inner sorts at each level.
        assert_eq!(relative_names, vec!["a.txt", "subdir/m.txt", "z.txt"]);
    }

    #[test]
    fn test_should_ignore() {
        assert!(should_ignore(Path::new(".git")));
        assert!(should_ignore(Path::new("target")));
        assert!(!should_ignore(Path::new("src")));
        assert!(!should_ignore(Path::new("Cargo.toml")));
    }

    #[test]
    fn test_normalize_path() {
        if cfg!(windows) {
            assert_eq!(normalize_path(Path::new("a\\b\\c")), "a/b/c");
        } else {
            assert_eq!(normalize_path(Path::new("a/b/c")), "a/b/c");
        }
    }
}
