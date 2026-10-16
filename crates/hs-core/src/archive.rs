//! Pack a folder into one file, in whichever format the user asked for.
//!
//! Used by the editor's **Download Zip** on a project folder and by the
//! SFTP pane's **Download Zipped** on a remote folder, which fetches it
//! first. It lives here rather than in either tool because tools never
//! depend on each other (ADR-004).

use std::io::Write;
use std::path::{Path, PathBuf};

/// The archive formats offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// `.zip`, deflate. What Windows opens without anything installed.
    #[default]
    Zip,
    /// `.tar.gz`: tar, then gzip. What the other end usually wants when
    /// the folder is going back to a Unix host.
    TarGz,
    /// `.tar`, no compression — for something already compressed.
    Tar,
}

impl Format {
    /// The extension this format adds, without a leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::TarGz => "tar.gz",
            Format::Tar => "tar",
        }
    }

    /// The name to offer in a save dialog for a folder called `name`.
    pub fn file_name(self, name: &str) -> String {
        format!("{name}.{}", self.extension())
    }

    pub const ALL: [Format; 3] = [Format::Zip, Format::TarGz, Format::Tar];
}

/// Everything under `dir`, as one archive at `target`. Returns how many
/// files went in.
///
/// Entries are stored under a top folder named after `dir`, so unpacking
/// never scatters files into the current directory. `.git` is skipped: it
/// is usually larger than the work and never what someone means by "send
/// me that folder".
pub fn pack(dir: &Path, target: &Path, format: Format) -> Result<usize, String> {
    let files = collect(dir)?;
    let base = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "folder".into());
    let out = std::fs::File::create(target).map_err(|e| format!("{}: {e}", target.display()))?;
    let out = std::io::BufWriter::new(out);
    match format {
        Format::Zip => write_zip(&files, dir, &base, out),
        Format::Tar => write_tar(&files, dir, &base, out),
        Format::TarGz => write_tar(
            &files,
            dir,
            &base,
            flate2::write::GzEncoder::new(out, flate2::Compression::default()),
        ),
    }?;
    Ok(files.len())
}

/// Every file under `dir`, in a stable order so two runs produce the same
/// archive.
fn collect(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .map_err(|e| format!("{}: {e}", d.display()))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                stack.push(p);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The path an entry takes inside the archive: always forward slashes,
/// always under one top folder.
fn entry_name(base: &str, dir: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(dir).unwrap_or(file);
    format!("{base}/{}", rel.to_string_lossy().replace('\\', "/"))
}

fn write_zip<W: Write + std::io::Seek>(
    files: &[PathBuf],
    dir: &Path,
    base: &str,
    out: W,
) -> Result<(), String> {
    let mut zip = zip::ZipWriter::new(out);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for f in files {
        let name = entry_name(base, dir, f);
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn write_tar<W: Write>(files: &[PathBuf], dir: &Path, base: &str, out: W) -> Result<(), String> {
    let mut tar = tar::Builder::new(out);
    for f in files {
        let name = entry_name(base, dir, f);
        let mut file = std::fs::File::open(f).map_err(|e| format!("{}: {e}", f.display()))?;
        tar.append_file(&name, &mut file)
            .map_err(|e| format!("{name}: {e}"))?;
    }
    // into_inner finishes the archive; for a gzip stream it also has to be
    // finished, which dropping the encoder does.
    tar.into_inner().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("README.md"), b"hello").unwrap();
        std::fs::write(root.join("src").join("main.rs"), b"fn main() {}").unwrap();
        std::fs::write(root.join(".git").join("HEAD"), b"ref: nope").unwrap();
        t
    }

    #[test]
    fn every_format_packs_the_tree_under_one_top_folder() {
        for format in Format::ALL {
            let t = tree();
            let dir = t.path().join("project");
            let out = t.path().join(format.file_name("project"));
            let n = pack(&dir, &out, format).unwrap();
            assert_eq!(n, 2, "{format:?} should hold the two real files");
            assert!(out.exists(), "{format:?} wrote nothing");
            assert!(
                std::fs::metadata(&out).unwrap().len() > 0,
                "{format:?} is empty"
            );
        }
    }

    #[test]
    fn a_zip_holds_the_paths_it_should_and_skips_git() {
        let t = tree();
        let dir = t.path().join("project");
        let out = t.path().join("project.zip");
        pack(&dir, &out, Format::Zip).unwrap();
        let f = std::fs::File::open(&out).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        let mut names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["project/README.md", "project/src/main.rs"]);
    }

    #[test]
    fn names_use_forward_slashes_under_the_folder_name() {
        let dir = Path::new("/tmp/project");
        let file = Path::new("/tmp/project/src/main.rs");
        assert_eq!(
            entry_name("project", dir, file),
            "project/src/main.rs",
            "an archive path is never a Windows path"
        );
    }

    #[test]
    fn each_format_names_its_file() {
        assert_eq!(Format::Zip.file_name("logs"), "logs.zip");
        assert_eq!(Format::TarGz.file_name("logs"), "logs.tar.gz");
        assert_eq!(Format::Tar.file_name("logs"), "logs.tar");
    }
}
