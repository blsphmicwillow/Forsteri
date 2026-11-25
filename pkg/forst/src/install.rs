//! Installing, recording and removing packages.
//!
//! Forsteri keeps one ledger, `/var/lib/forst/db`, no matter which codename a
//! package came from. Every installed package owns a `.meta` (identity, deps)
//! and a `.files` list (tag + path), which is what `remove` and `files` walk.
//!
//! Packages are extracted, never "installed" by their upstream tooling: no
//! dpkg/rpm scriptlets run, no second libc is installed. For base codenames the
//! file set is the Arch payload, which is exactly what a Forsteri system needs;
//! for asset codenames this is the safe behaviour.

use crate::config::{Format, Repo};
use crate::index::Package;
use crate::util::{self, err, Error, FsEntry, FsKind, Result};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// ledger
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Meta {
    pub name: String,
    pub version: String,
    pub codename: String,
    pub format: String,
    pub arch: String,
    pub installed_at: u64,
    pub depends: Vec<String>,
    pub provides: Vec<String>,
    pub description: String,
}

pub fn db_dir(db: &Path) -> PathBuf {
    db.join("db")
}

pub fn meta_path(db: &Path, name: &str) -> PathBuf {
    db_dir(db).join(format!("{name}.meta"))
}

pub fn files_path(db: &Path, name: &str) -> PathBuf {
    db_dir(db).join(format!("{name}.files"))
}

impl Meta {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("name={}\n", self.name));
        out.push_str(&format!("version={}\n", self.version));
        out.push_str(&format!("codename={}\n", self.codename));
        out.push_str(&format!("format={}\n", self.format));
        out.push_str(&format!("arch={}\n", self.arch));
        out.push_str(&format!("installed_at={}\n", self.installed_at));
        out.push_str(&format!("depends={}\n", self.depends.join(" ")));
        out.push_str(&format!("provides={}\n", self.provides.join(" ")));
        out.push_str(&format!("description={}\n", self.description));
        out
    }
}

fn parse_meta(text: &str) -> Meta {
    let mut meta = Meta {
        name: String::new(),
        version: String::new(),
        codename: String::new(),
        format: String::new(),
        arch: String::new(),
        installed_at: 0,
        depends: Vec::new(),
        provides: Vec::new(),
        description: String::new(),
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => meta.name = value.trim().to_string(),
            "version" => meta.version = value.trim().to_string(),
            "codename" => meta.codename = value.trim().to_string(),
            "format" => meta.format = value.trim().to_string(),
            "arch" => meta.arch = value.trim().to_string(),
            "installed_at" => meta.installed_at = value.trim().parse().unwrap_or(0),
            "depends" => meta.depends = value.split_whitespace().map(str::to_string).collect(),
            "provides" => meta.provides = value.split_whitespace().map(str::to_string).collect(),
            "description" => meta.description = value.trim().to_string(),
            _ => {}
        }
    }
    meta
}

pub fn installed(db: &Path) -> Result<Vec<Meta>> {
    let dir = db_dir(db);
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("meta") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let meta = parse_meta(&text);
        if !meta.name.is_empty() {
            out.push(meta);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn get_installed(db: &Path, name: &str) -> Option<Meta> {
    let path = meta_path(db, name);
    let text = fs::read_to_string(path).ok()?;
    let meta = parse_meta(&text);
    if meta.name.is_empty() {
        None
    } else {
        Some(meta)
    }
}

pub fn list_files(db: &Path, name: &str) -> Result<Vec<FsEntry>> {
    let path = files_path(db, name);
    if !path.exists() {
        return err(format!("{name} is not installed (no {})", path.display()));
    }
    let text = fs::read_to_string(&path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((tag, rest)) = line.split_once('\t') else {
            continue;
        };
        let kind = FsKind::from_tag(tag.chars().next().unwrap_or('f'));
        out.push(FsEntry {
            kind,
            path: rest.to_string(),
        });
    }
    Ok(out)
}

fn write_ledger(db: &Path, meta: &Meta, files: &[FsEntry]) -> Result<()> {
    let dir = db_dir(db);
    fs::create_dir_all(&dir)?;
    let mut text = String::new();
    for entry in files {
        text.push_str(&format!("{}\t{}\n", entry.kind.tag(), entry.path));
    }
    fs::write(files_path(db, &meta.name), text)?;
    fs::write(meta_path(db, &meta.name), meta.to_text())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// install
// ---------------------------------------------------------------------------

pub struct InstallReport {
    pub files: usize,
    pub downloaded: bool,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn package_cache_path(cache: &Path, pkg: &Package) -> PathBuf {
    let basename = Path::new(&pkg.filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&pkg.name)
        .to_string();
    cache.join("pkgs").join(&pkg.codename).join(basename)
}

fn fetch_package(
    repo: &Repo,
    pkg: &Package,
    cache: &Path,
    verbose: bool,
) -> Result<(PathBuf, bool)> {
    let dest = package_cache_path(cache, pkg);
    let mut downloaded = false;
    let mut needs_fetch = !dest.exists();
    if let Ok(md) = fs::metadata(&dest) {
        if pkg.size > 0 && md.len() != pkg.size {
            needs_fetch = true;
        }
    }
    if needs_fetch {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let part = dest.with_extension("part");
        let url = repo.package_url(&pkg.filename);
        if verbose {
            eprintln!("forst: fetching {url}");
        }
        crate::sync::download(&url, &part, true)
            .map_err(|e| Error::new(format!("{}: {e}", pkg.spec())))?;
        fs::rename(&part, &dest)?;
        downloaded = true;
    }

    if pkg.sha256.len() == 64 {
        let actual = util::sha256_file(&dest)?;
        if !actual.eq_ignore_ascii_case(&pkg.sha256) {
            let _ = fs::remove_file(&dest);
            return err(format!(
                "{}: checksum mismatch\n  expected {}\n  actual   {}",
                pkg.spec(),
                pkg.sha256,
                actual
            ));
        }
    } else if !pkg.sha256.is_empty() {
        util::warn(&format!(
            "{}: no sha256 in the index, skipping verification",
            pkg.spec()
        ));
    }
    Ok((dest, downloaded))
}

fn extract(
    repo: &Repo,
    pkg: &Package,
    archive: &Path,
    root: &Path,
    tmpdir: &Path,
) -> Result<Vec<FsEntry>> {
    fs::create_dir_all(root)?;
    match repo.format {
        Format::Pacman => {
            let raw = util::decompress_path(archive, tmpdir)?;
            let mut reader = BufReader::new(File::open(raw.path())?);
            util::tar_extract(&mut reader, root, true)
        }
        Format::Debian => util::deb_extract(archive, root, tmpdir),
        Format::Rpm => util::rpm_extract(archive, root, tmpdir),
    }
    .map_err(|e| Error::new(format!("{}: {e}", pkg.spec())))
}

pub fn install_package(
    repo: &Repo,
    pkg: &Package,
    root: &Path,
    cache: &Path,
    db: &Path,
    verbose: bool,
) -> Result<InstallReport> {
    let (archive, downloaded) = fetch_package(repo, pkg, cache, verbose)?;
    let tmpdir = cache.join("tmp");
    fs::create_dir_all(&tmpdir)?;

    let files = extract(repo, pkg, &archive, root, &tmpdir)?;
    if files.is_empty() {
        util::warn(&format!("{}: archive contained no files", pkg.spec()));
    }

    let meta = Meta {
        name: pkg.name.clone(),
        version: pkg.version.clone(),
        codename: pkg.codename.clone(),
        format: repo.format.as_str().to_string(),
        arch: pkg.arch.clone(),
        installed_at: now_unix(),
        depends: pkg.depends.clone(),
        provides: pkg.provides.clone(),
        description: pkg.description.clone(),
    };
    write_ledger(db, &meta, &files)?;

    Ok(InstallReport {
        files: files.len(),
        downloaded,
    })
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

pub fn remove_package(db: &Path, root: &Path, name: &str) -> Result<usize> {
    let files = list_files(db, name)?;
    let mut removed = 0;
    // Reverse archive order: files before the directories that contain them.
    for entry in files.iter().rev() {
        let Some(rel) = util::safe_rel(&entry.path) else {
            continue;
        };
        let full = root.join(rel);
        match entry.kind {
            FsKind::Dir => {
                // Only remove if empty; shared directories stay.
                let _ = fs::remove_dir(&full);
            }
            _ => match fs::symlink_metadata(&full) {
                Ok(md) => {
                    if !md.is_dir() {
                        if fs::remove_file(&full).is_ok() {
                            removed += 1;
                        }
                    }
                }
                Err(_) => {}
            },
        }
    }
    let _ = fs::remove_file(meta_path(db, name));
    let _ = fs::remove_file(files_path(db, name));
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_roundtrip() {
        let dir = std::env::temp_dir().join(format!("forst-meta-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let meta = Meta {
            name: "acl".into(),
            version: "2.4.0-1".into(),
            codename: "colony".into(),
            format: "pacman".into(),
            arch: "x86_64".into(),
            installed_at: 42,
            depends: vec!["glibc".into()],
            provides: vec!["acl-tools".into()],
            description: "access control = important".into(),
        };
        write_ledger(
            &dir,
            &meta,
            &[FsEntry {
                kind: FsKind::File,
                path: "usr/bin/getfacl".into(),
            }],
        )
        .unwrap();
        let back = get_installed(&dir, "acl").unwrap();
        assert_eq!(back.depends, vec!["glibc"]);
        assert_eq!(back.description, "access control = important");
        let files = list_files(&dir, "acl").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].kind, FsKind::File);
        fs::remove_dir_all(&dir).ok();
    }
}
