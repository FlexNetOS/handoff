use crate::fs_utils::{relative_path, stable_walk, write_string_preserving_existing};
use crate::model::ManifestEntry;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

pub fn generate_manifest(root: impl AsRef<Path>) -> Result<Vec<ManifestEntry>, String> {
    let root = root.as_ref();
    if !root.exists() || !root.is_dir() {
        return Err(format!(
            "manifest root is not a directory: {}",
            root.display()
        ));
    }

    let mut entries = Vec::new();
    for abs in stable_walk(root).map_err(|e| format!("walk failed: {e}"))? {
        let metadata = match fs::metadata(&abs) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let digest =
            fnv1a64_file(&abs).map_err(|e| format!("hash failed for {}: {e}", abs.display()))?;
        entries.push(ManifestEntry {
            path: relative_path(root, &abs),
            size_bytes: metadata.len(),
            fnv1a64: format!("{digest:016x}"),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

pub fn manifest_tsv(entries: &[ManifestEntry]) -> String {
    let mut out = String::from("path\tsize_bytes\tfnv1a64\n");
    for entry in entries {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            entry.path, entry.size_bytes, entry.fnv1a64
        ));
    }
    out
}

pub fn write_manifest(
    root: impl AsRef<Path>,
    out: impl AsRef<Path>,
) -> Result<Vec<ManifestEntry>, String> {
    let entries = generate_manifest(root)?;
    write_string_preserving_existing(out.as_ref(), &manifest_tsv(&entries))
        .map_err(|e| format!("failed to write manifest: {e}"))?;
    Ok(entries)
}

fn fnv1a64_file(path: &Path) -> io::Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_tsv_has_header() {
        let out = manifest_tsv(&[]);
        assert_eq!(out, "path\tsize_bytes\tfnv1a64\n");
    }
}
