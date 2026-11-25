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
use std::io::{BufReader, Write};
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
        let mut fields = line.splitn(3, '\t');
        let Some(tag) = fields.next() else {
            continue;
        };
        let Some(path) = fields.next() else {
            continue;
        };
        let kind = FsKind::from_tag(tag.chars().next().unwrap_or('f'));
        out.push(FsEntry {
            kind,
            path: path.to_string(),
            sha256: fields.next().unwrap_or("").trim().to_string(),
        });
    }
    Ok(out)
}

fn write_ledger(db: &Path, meta: &Meta, files: &[FsEntry]) -> Result<()> {
    let dir = db_dir(db);
    fs::create_dir_all(&dir)?;
    let mut text = String::new();
    for entry in files {
        text.push_str(&format!("{}\t{}", entry.kind.tag(), entry.path));
        if !entry.sha256.is_empty() {
            text.push('\t');
            text.push_str(&entry.sha256);
        }
        text.push('\n');
    }
    // The .meta file is the commit marker: write it last, and write both files
    // through a temporary name so a kill cannot expose a half-written ledger.
    write_atomic(&files_path(db, &meta.name), text.as_bytes())?;
    write_atomic(&meta_path(db, &meta.name), meta.to_text().as_bytes())?;
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ledger");
    let tmp = path.with_file_name(format!(".{name}.tmp.{}", std::process::id()));
    {
        let mut file = File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
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

    if pkg.sha256.len() != 64 || !pkg.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        let _ = fs::remove_file(&dest);
        return err(format!(
            "{}: the signed index has no valid SHA256; refusing the package",
            pkg.spec()
        ));
    }
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
    if repo.requires_package_signature() {
        let keyring = repo.keyring_path()?;
        if repo.format == Format::Pacman {
            let sig = PathBuf::from(format!("{}.sig", dest.display()));
            if !sig.exists() {
                let url = format!("{}.sig", repo.package_url(&pkg.filename));
                crate::sync::download(&url, &sig, true)
                    .map_err(|e| Error::new(format!("{}: signature: {e}", pkg.spec())))?;
            }
            crate::sign::verify_detached(&dest, &sig, &keyring)
                .map_err(|e| Error::new(format!("{}: {e}", pkg.spec())))?;
        } else {
            crate::sign::verify_rpm_package(&dest, &keyring)
                .map_err(|e| Error::new(format!("{}: {e}", pkg.spec())))?;
        }
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

/// Hash every regular file in a staged package, so the ledger can later prove
/// that what is on disk is what was installed.
fn hash_entries(stage: &Path, files: &mut [FsEntry]) -> Result<()> {
    for entry in files.iter_mut() {
        if !matches!(entry.kind, FsKind::File | FsKind::Hardlink) {
            continue;
        }
        let Some(rel) = util::safe_rel(&entry.path) else {
            return err(format!("unsafe staged path {:?}", entry.path));
        };
        entry.sha256 = util::sha256_file(&stage.join(rel))?;
    }
    Ok(())
}

/// Move a verified staging tree into the live root. Directories are created
/// first; each file or link is replaced with a rename, which is atomic on the
/// same filesystem (the staging tree always lives under the target root).
fn commit_stage(stage: &Path, root: &Path, files: &[FsEntry]) -> Result<()> {
    for entry in files.iter().filter(|e| e.kind == FsKind::Dir) {
        let Some(rel) = util::safe_rel(&entry.path) else {
            continue;
        };
        let dest = root.join(rel);
        match fs::symlink_metadata(&dest) {
            Ok(md) if md.is_dir() => {}
            Ok(_) => return err(format!("{} is not a directory", dest.display())),
            Err(_) => fs::create_dir_all(&dest)?,
        }
    }
    for entry in files.iter().filter(|e| e.kind != FsKind::Dir) {
        let Some(rel) = util::safe_rel(&entry.path) else {
            return err(format!("unsafe staged path {:?}", entry.path));
        };
        let src = stage.join(&rel);
        let dest = root.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::symlink_metadata(&dest) {
            Ok(md) if md.is_dir() && !md.file_type().is_symlink() => {
                fs::remove_dir_all(&dest)?;
            }
            Ok(_) => {
                fs::remove_file(&dest)?;
            }
            Err(_) => {}
        }
        fs::rename(&src, &dest)
            .map_err(|e| Error::new(format!("cannot commit {}: {e}", dest.display())))?;
    }
    Ok(())
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

    let stage = root
        .join(".forst-stage")
        .join(format!("{}-{}", pkg.name, std::process::id()));
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    fs::create_dir_all(&stage)?;

    let result = (|| -> Result<Vec<FsEntry>> {
        let mut files = extract(repo, pkg, &archive, &stage, &tmpdir)?;
        if files.is_empty() {
            util::warn(&format!("{}: archive contained no files", pkg.spec()));
        }
        // Verify the staged tree before anything reaches the live root.
        hash_entries(&stage, &mut files)?;
        commit_stage(&stage, root, &files)?;
        Ok(files)
    })();

    let files = match result {
        Ok(files) => files,
        Err(e) => {
            fs::remove_dir_all(&stage).ok();
            return Err(e);
        }
    };
    fs::remove_dir_all(&stage).ok();

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
    // This is the commit point. Before it, an interrupted install leaves old
    // ledger entries intact; after it, every file has already been moved.
    write_ledger(db, &meta, &files)?;

    Ok(InstallReport {
        files: files.len(),
        downloaded,
    })
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Drift {
    pub package: String,
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct VerifyReport {
    pub packages: usize,
    pub checked: usize,
    pub unhashed: usize,
    pub drift: Vec<Drift>,
}

/// Re-hash every installed file that has a recorded digest. Missing files and
/// content changes are drift; directories and symlinks are checked for
/// existence. Ledger entries from older forst versions have no digest and are
/// counted as unhashed rather than silently called clean.
pub fn verify(db: &Path, root: &Path, only: &[String]) -> Result<VerifyReport> {
    let metas = installed(db)?;
    let selected: Vec<Meta> = if only.is_empty() {
        metas
    } else {
        let mut out = Vec::new();
        for name in only {
            let meta = get_installed(db, name)
                .ok_or_else(|| Error::new(format!("{name} is not installed")))?;
            out.push(meta);
        }
        out
    };

    let mut report = VerifyReport::default();
    for meta in &selected {
        report.packages += 1;
        for entry in list_files(db, &meta.name)? {
            let Some(rel) = util::safe_rel(&entry.path) else {
                report.drift.push(Drift {
                    package: meta.name.clone(),
                    path: entry.path.clone(),
                    reason: "unsafe ledger path".to_string(),
                });
                continue;
            };
            let full = root.join(&rel);
            if matches!(entry.kind, FsKind::Dir | FsKind::Symlink) {
                if fs::symlink_metadata(&full).is_err() {
                    report.drift.push(Drift {
                        package: meta.name.clone(),
                        path: format!("/{}", rel.display()),
                        reason: "missing".to_string(),
                    });
                }
                continue;
            }
            if entry.sha256.is_empty() {
                report.unhashed += 1;
                continue;
            }
            if fs::symlink_metadata(&full).is_err() {
                report.drift.push(Drift {
                    package: meta.name.clone(),
                    path: format!("/{}", rel.display()),
                    reason: "missing".to_string(),
                });
                continue;
            }
            report.checked += 1;
            let actual = util::sha256_file(&full)?;
            if !actual.eq_ignore_ascii_case(&entry.sha256) {
                report.drift.push(Drift {
                    package: meta.name.clone(),
                    path: format!("/{}", rel.display()),
                    reason: "modified".to_string(),
                });
            }
        }
    }
    Ok(report)
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
                sha256: "ab".repeat(32),
            }],
        )
        .unwrap();
        let back = get_installed(&dir, "acl").unwrap();
        assert_eq!(back.depends, vec!["glibc"]);
        assert_eq!(back.description, "access control = important");
        let files = list_files(&dir, "acl").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].kind, FsKind::File);
        assert_eq!(files[0].sha256, "ab".repeat(32));
        fs::remove_dir_all(&dir).ok();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forst-{name}-{}-{}",
            std::process::id(),
            now_unix()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn verify_finds_modified_and_missing_files() {
        let base = scratch("verify");
        let root = base.join("root");
        let db = base.join("db");
        let tool = root.join("usr/bin/getfacl");
        fs::create_dir_all(tool.parent().unwrap()).unwrap();
        fs::write(&tool, b"original").unwrap();
        let digest = util::sha256_file(&tool).unwrap();
        let meta = Meta {
            name: "acl".into(),
            version: "1".into(),
            codename: "colony".into(),
            format: "pacman".into(),
            arch: "x86_64".into(),
            installed_at: 1,
            depends: vec![],
            provides: vec![],
            description: String::new(),
        };
        write_ledger(
            &db,
            &meta,
            &[FsEntry {
                kind: FsKind::File,
                path: "usr/bin/getfacl".into(),
                sha256: digest,
            }],
        )
        .unwrap();

        let clean = verify(&db, &root, &[]).unwrap();
        assert!(clean.drift.is_empty());
        assert_eq!(clean.checked, 1);

        fs::write(&tool, b"tampered").unwrap();
        let changed = verify(&db, &root, &[]).unwrap();
        assert_eq!(changed.drift.len(), 1);
        assert_eq!(changed.drift[0].reason, "modified");

        fs::remove_file(&tool).unwrap();
        let missing = verify(&db, &root, &[]).unwrap();
        assert_eq!(missing.drift[0].reason, "missing");
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn commit_stage_moves_only_after_extraction() {
        let base = scratch("stage");
        let root = base.join("root");
        let stage = root.join(".forst-stage/acl");
        fs::create_dir_all(stage.join("usr/bin")).unwrap();
        fs::write(stage.join("usr/bin/getfacl"), b"payload").unwrap();
        let entries = vec![
            FsEntry {
                kind: FsKind::File,
                path: "usr/bin/getfacl".into(),
                sha256: String::new(),
            },
        ];
        // Nothing has reached the live root before commit_stage.
        assert!(!root.join("usr/bin/getfacl").exists());
        commit_stage(&stage, &root, &entries).unwrap();
        assert_eq!(fs::read(root.join("usr/bin/getfacl")).unwrap(), b"payload");
        // The ledger is written separately, and is the commit marker.
        assert!(install_meta_files(&base.join("db")).is_empty());
        fs::remove_dir_all(&base).ok();
    }

    fn install_meta_files(db: &Path) -> Vec<String> {
        fs::read_dir(db_dir(db))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().to_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn atomic_ledger_writes_leave_no_partial_files() {
        let base = scratch("atomic");
        let meta = Meta {
            name: "acl".into(),
            version: "1".into(),
            codename: "colony".into(),
            format: "pacman".into(),
            arch: "x86_64".into(),
            installed_at: 1,
            depends: vec![],
            provides: vec![],
            description: String::new(),
        };
        write_ledger(&base, &meta, &[]).unwrap();
        let names = install_meta_files(&base);
        assert!(names.contains(&"acl.meta".to_string()));
        assert!(names.contains(&"acl.files".to_string()));
        assert!(!names.iter().any(|n| n.contains(".tmp.")));
        fs::remove_dir_all(&base).ok();
    }
}
