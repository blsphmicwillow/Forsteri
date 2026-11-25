//! Fetching repository indexes.
//!
//! Every index is authenticated before it is parsed. A mirror that serves a
//! missing signature, an unknown key, a bad signature or an index whose digest
//! disagrees with the signed metadata cannot poison the cache.

use crate::config::{Format, Repo};
use crate::index::{self, Index};
use crate::sign;
use crate::util::{err, human_size, Error, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Download `url` to `dest` with curl (always present on the base system, and
/// it speaks https/file:// for mirrors).
pub fn download(url: &str, dest: &Path, quiet: bool) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut cmd = Command::new("curl");
    cmd.args([
        "-fsSL",
        "--retry",
        "3",
        "--retry-delay",
        "2",
        "--connect-timeout",
        "20",
        "-o",
    ])
    .arg(dest)
    .arg(url);
    if quiet {
        cmd.arg("-sS");
    }
    let out = cmd
        .output()
        .map_err(|e| Error::new(format!("cannot run curl: {e}")))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if msg.is_empty() {
            format!("curl exited with {}", out.status)
        } else {
            msg
        };
        return err(format!("{url}: {msg}"));
    }
    Ok(())
}

fn read_url(url: &str, tmpdir: &Path, name: &str) -> Result<Vec<u8>> {
    let dest = tmpdir.join(name);
    download(url, &dest, true)?;
    let data = fs::read(&dest)?;
    fs::remove_file(&dest).ok();
    Ok(data)
}

fn verify_detached_file(repo: &Repo, data: &Path, sig: &Path) -> Result<()> {
    let keyring = repo.keyring_path()?;
    sign::verify_detached(data, sig, &keyring)
        .map_err(|e| Error::new(format!("{}: {e}", repo.codename)))
}

fn fetch_pacman(repo: &Repo, tmpdir: &Path) -> Result<Vec<u8>> {
    let raw = read_url(&repo.index_url(), tmpdir, "index.db")?;
    // Arch's mirrors no longer publish core.db.sig; pacman trusts package
    // signatures (checked at install). If a mirror does publish a database
    // signature, verify it rather than ignoring it.
    match read_url(&repo.signature_url(), tmpdir, "index.db.sig") {
        Ok(sig) => {
            let index_path = tmpdir.join("index.db");
            fs::write(&index_path, &raw)?;
            let sig_path = tmpdir.join("index.db.sig");
            fs::write(&sig_path, &sig)?;
            verify_detached_file(repo, &index_path, &sig_path)?;
            fs::remove_file(index_path).ok();
            fs::remove_file(sig_path).ok();
        }
        Err(_) => {
            crate::util::warn(&format!(
                "{}: mirror has no database signature; Arch verifies package signatures at install",
                repo.codename
            ));
        }
    }
    Ok(raw)
}

fn fetch_debian(repo: &Repo, tmpdir: &Path, verbose: bool) -> Result<Vec<u8>> {
    let inrelease_path = tmpdir.join("InRelease");
    download(&repo.signature_url(), &inrelease_path, true)?;
    let keyring = repo.keyring_path()?;
    sign::verify_inline(&inrelease_path, &keyring)
        .map_err(|e| Error::new(format!("{}: InRelease: {e}", repo.codename)))?;
    let release = fs::read_to_string(&inrelease_path)?;
    let relative = format!(
        "{}/binary-{}/Packages.xz",
        repo.component.trim_matches('/'),
        repo.arch
    );
    let expected = sign::debian_sha256(&release, &relative).ok_or_else(|| {
        Error::new(format!(
            "{}: signed InRelease has no SHA256 for {relative}",
            repo.codename
        ))
    })?;
    if verbose {
        eprintln!(
            "forst: {}: signed {relative} sha256 {}",
            repo.codename, expected
        );
    }
    let raw = read_url(&repo.index_url(), tmpdir, "Packages.xz")?;
    let index_path = tmpdir.join("Packages.xz");
    fs::write(&index_path, &raw)?;
    sign::verify_sha256_file(&index_path, &expected)
        .map_err(|e| Error::new(format!("{}: {e}", repo.codename)))?;
    fs::remove_file(index_path).ok();
    Ok(raw)
}

fn fetch_rpm(repo: &Repo, tmpdir: &Path, verbose: bool) -> Result<Vec<u8>> {
    let repomd = read_url(&repo.index_url(), tmpdir, "repomd.xml")?;
    let repomd_path = tmpdir.join("repomd.xml");
    fs::write(&repomd_path, &repomd)?;
    // Fedora deliberately does not publish repomd.xml.asc; the package
    // signature is its trust anchor. If a mirror does publish one, verify it
    // and refuse a bad signature rather than falling through.
    match read_url(&repo.signature_url(), tmpdir, "repomd.xml.asc") {
        Ok(asc) => {
            let asc_path = tmpdir.join("repomd.xml.asc");
            fs::write(&asc_path, &asc)?;
            verify_detached_file(repo, &repomd_path, &asc_path)?;
            fs::remove_file(asc_path).ok();
        }
        Err(_) => {
            crate::util::warn(&format!(
                "{}: mirror has no repomd.xml.asc (Fedora's normal model); packages must carry valid RPM signatures",
                repo.codename
            ));
        }
    }
    fs::remove_file(repomd_path).ok();

    let text = String::from_utf8_lossy(&repomd).into_owned();
    let (href, expected) = sign::repomd_primary(&text).ok_or_else(|| {
        Error::new(format!(
            "{}: signed repomd.xml has no sha256 primary entry",
            repo.codename
        ))
    })?;
    let url = format!("{}/{}", repo.url.trim_end_matches('/'), href.trim_start_matches('/'));
    if verbose {
        eprintln!("forst: {}: signed primary metadata {url}", repo.codename);
    }
    let raw = read_url(&url, tmpdir, "primary.xml")?;
    let primary_path = tmpdir.join("primary.xml");
    fs::write(&primary_path, &raw)?;
    sign::verify_sha256_file(&primary_path, &expected)
        .map_err(|e| Error::new(format!("{}: {e}", repo.codename)))?;
    fs::remove_file(primary_path).ok();
    Ok(raw)
}

/// Fetch, authenticate and normalise one codename into its cached index.
pub fn sync_repo(repo: &Repo, cache: &Path, verbose: bool) -> Result<Index> {
    let tmpdir = cache
        .join("tmp")
        .join(format!("{}-{}", std::process::id(), repo.codename));
    fs::create_dir_all(&tmpdir)?;

    let (packages, raw_len) = match repo.format {
        Format::Pacman => {
            let raw = fetch_pacman(repo, &tmpdir)?;
            let len = raw.len();
            (index::parse(repo, &raw)?, len)
        }
        Format::Debian => {
            let raw = fetch_debian(repo, &tmpdir, verbose)?;
            let len = raw.len();
            (index::parse(repo, &raw)?, len)
        }
        Format::Rpm => {
            let raw = fetch_rpm(repo, &tmpdir, verbose)?;
            let xml = crate::util::decompress_bytes(&raw)?;
            let xml = String::from_utf8_lossy(&xml).into_owned();
            (index::parse_rpm_primary(&xml, repo), raw.len())
        }
    };
    fs::remove_dir_all(&tmpdir).ok();

    if packages.is_empty() {
        return err(format!(
            "{}: index parsed to zero packages ({})",
            repo.codename,
            repo.index_url()
        ));
    }

    if verbose {
        eprintln!("forst: {}: signed index {}", repo.codename, human_size(raw_len as u64));
    }

    let index = Index::new(&repo.codename, packages);
    index.save(&repo.index_path(cache))?;
    Ok(index)
}

/// `forst sync`: every enabled codename, individual failures are reported but
/// do not stop the others (a missing `nest` mirror should not block updates).
pub struct SyncOutcome {
    pub codename: String,
    pub count: usize,
    pub error: Option<String>,
}

pub fn sync_all(repos: &[Repo], cache: &Path, only: &[String], verbose: bool) -> Vec<SyncOutcome> {
    let mut outcomes = Vec::new();
    for repo in repos.iter().filter(|r| r.enabled) {
        if !only.is_empty() && !only.iter().any(|c| c.eq_ignore_ascii_case(&repo.codename)) {
            continue;
        }
        match sync_repo(repo, cache, verbose) {
            Ok(index) => outcomes.push(SyncOutcome {
                codename: repo.codename.clone(),
                count: index.packages.len(),
                error: None,
            }),
            Err(e) => outcomes.push(SyncOutcome {
                codename: repo.codename.clone(),
                count: 0,
                error: Some(e.to_string()),
            }),
        }
    }
    outcomes
}
