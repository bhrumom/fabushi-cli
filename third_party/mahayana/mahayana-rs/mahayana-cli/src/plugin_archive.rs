use flate2::Compression;
use flate2::GzBuilder;
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use tar::{Builder, EntryType, Header};

#[derive(Debug, thiserror::Error)]
pub enum PluginArchiveError {
    #[error("plugin root is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("plugin archive rejects symbolic links: {0}")]
    SymbolicLink(PathBuf),
    #[error("plugin archive exceeds {limit} bytes of source files ({actual} bytes)")]
    TooLarge { limit: u64, actual: u64 },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Packs one plugin directory into a deterministic gzip-compressed tarball.
///
/// The product-owned packer deliberately rejects symlinks instead of following
/// them, strips host-specific ownership/timestamps, and enforces a source-size
/// ceiling before reading file contents. `.git` and `target` are never shipped.
pub fn pack_plugin_bundle_tar_gz(
    root: &Path,
    max_source_bytes: u64,
) -> Result<Vec<u8>, PluginArchiveError> {
    if !root.is_dir() {
        return Err(PluginArchiveError::InvalidRoot(root.to_path_buf()));
    }

    let mut entries = Vec::new();
    collect_entries(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let source_bytes = entries
        .iter()
        .filter_map(|(_, path, is_dir)| (!*is_dir).then_some(path))
        .try_fold(0_u64, |total, path| {
            let len = fs::metadata(path)?.len();
            total
                .checked_add(len)
                .ok_or_else(|| io::Error::other("plugin source size overflow"))
        })?;
    if source_bytes > max_source_bytes {
        return Err(PluginArchiveError::TooLarge {
            limit: max_source_bytes,
            actual: source_bytes,
        });
    }

    let encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    let mut builder = Builder::new(encoder);

    for (relative, path, is_dir) in entries {
        let mut header = Header::new_gnu();
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        if is_dir {
            header.set_entry_type(EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            header.set_cksum();
            builder.append_data(&mut header, relative, io::empty())?;
        } else {
            let metadata = fs::metadata(&path)?;
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(metadata.len());
            header.set_cksum();
            let mut file = File::open(path)?;
            builder.append_data(&mut header, relative, &mut file)?;
        }
    }

    builder.finish()?;
    let encoder = builder.into_inner()?;
    Ok(encoder.finish()?)
}

fn collect_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<(PathBuf, PathBuf, bool)>,
) -> Result<(), PluginArchiveError> {
    let mut children = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());

    for child in children {
        let name = child.file_name();
        if name == ".git" || name == "target" {
            continue;
        }
        let path = child.path();
        let file_type = child.file_type()?;
        if file_type.is_symlink() {
            return Err(PluginArchiveError::SymbolicLink(path));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PluginArchiveError::InvalidRoot(path.clone()))?
            .to_path_buf();
        if file_type.is_dir() {
            entries.push((relative, path.clone(), true));
            collect_entries(root, &path, entries)?;
        } else if file_type.is_file() {
            entries.push((relative, path, false));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mahayana-plugin-archive-{name}-{nonce}"));
        fs::create_dir_all(root.join("nested")).expect("fixture directory");
        fs::write(root.join("plugin.json"), b"{}\n").expect("fixture manifest");
        fs::write(root.join("nested/tool.txt"), b"native\n").expect("fixture file");
        root
    }

    #[test]
    fn archive_is_deterministic_and_product_owned() {
        let root = fixture("deterministic");
        let first = pack_plugin_bundle_tar_gz(&root, 1024).expect("first archive");
        let second = pack_plugin_bundle_tar_gz(&root, 1024).expect("second archive");
        assert_eq!(first, second);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn archive_enforces_source_size_before_packaging() {
        let root = fixture("limit");
        let error = pack_plugin_bundle_tar_gz(&root, 1).expect_err("size limit");
        assert!(matches!(error, PluginArchiveError::TooLarge { .. }));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
