//! Fetching repository indexes.

use crate::config::{Format, Repo};
use crate::index::{self, Index};
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

fn read_url(url: &str, tmpdir: &Path) -> Result<Vec<u8>> {
    let dest = tmpdir.join("forst-url.tmp");
    download(url, &dest, true)?;
    let data = fs::read(&dest)?;
    fs::remove_file(&dest).ok();
    Ok(data)
}

/// Fetch and normalise one codename into its cached index.
pub fn sync_repo(repo: &Repo, cache: &Path, verbose: bool) -> Result<Index> {
    let tmpdir = cache.join("tmp");
    fs::create_dir_all(&tmpdir)?;

    let packages = match repo.format {
        Format::Rpm => {
            let repomd = read_url(&repo.index_url(), &tmpdir)?;
            let repomd = String::from_utf8_lossy(&repomd).into_owned();
            let href = index::rpm_primary_location(&repomd).ok_or_else(|| {
                Error::new(format!(
                    "{}: no <data type=\"primary\"> in repomd.xml",
                    repo.codename
                ))
            })?;
            let url = format!("{}/{}", repo.url, href);
            if verbose {
                eprintln!("forst: {}: primary metadata {url}", repo.codename);
            }
            let raw = read_url(&url, &tmpdir)?;
            let xml = crate::util::decompress_bytes(&raw)?;
            let xml = String::from_utf8_lossy(&xml).into_owned();
            index::parse_rpm_primary(&xml, repo)
        }
        _ => {
            let raw = read_url(&repo.index_url(), &tmpdir)?;
            if verbose {
                eprintln!(
                    "forst: {}: index {} ({}, {})",
                    repo.codename,
                    repo.index_url(),
                    crate::util::compression_name(&raw),
                    human_size(raw.len() as u64)
                );
            }
            index::parse(repo, &raw)?
        }
    };

    if packages.is_empty() {
        return err(format!(
            "{}: index parsed to zero packages ({})",
            repo.codename,
            repo.index_url()
        ));
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
