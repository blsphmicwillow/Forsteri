//! Trust for repository metadata.
//!
//! forst verifies signed metadata before it believes a single package
//! checksum, then binds the downloaded index to the checksum named inside that
//! signed metadata. This is the same trust shape pacman, apt and dnf use:
//!
//! * pacman: `core.db` + detached `core.db.sig`
//! * Debian: clearsigned `InRelease`, whose SHA256 section names `Packages.xz`
//! * Fedora: `repomd.xml` + detached `repomd.xml.asc`, whose primary data
//!   element names the primary XML checksum
//!
//! There is deliberately no "unsigned is fine" path. A missing signature,
//! missing keyring, missing checksum or bad checksum is an error.

use crate::util::{self, err, Error, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

fn run_gpgv(keyring: &Path, args: &[&str]) -> Result<()> {
    if !keyring.is_file() {
        return err(format!(
            "signature keyring {} is missing; refusing unsigned metadata",
            keyring.display()
        ));
    }
    let out = Command::new("gpgv")
        .arg("--keyring")
        .arg(keyring)
        .arg("--status-fd")
        .arg("1")
        .args(args)
        .output()
        .map_err(|e| Error::new(format!("cannot run gpgv: {e}")))?;
    let status = String::from_utf8_lossy(&out.stdout);
    let valid = status
        .lines()
        .any(|line| line.starts_with("[GNUPG:] VALIDSIG "));
    // Debian signs InRelease with several archive keys; gpgv exits non-zero
    // for signatures it cannot check, but one VALIDSIG from our keyring is
    // enough to authenticate the document. No VALIDSIG is a hard failure.
    if !valid {
        let msg = String::from_utf8_lossy(&out.stderr);
        let msg = msg.trim();
        let msg = if msg.is_empty() {
            format!("gpgv exited with {}", out.status)
        } else {
            msg.to_string()
        };
        return err(format!("signature verification failed: {msg}"));
    }
    Ok(())
}

/// Verify a detached signature over an existing file.
pub fn verify_detached(data: &Path, signature: &Path, keyring: &Path) -> Result<()> {
    if !signature.is_file() {
        return err(format!(
            "detached signature {} is missing; refusing unsigned metadata",
            signature.display()
        ));
    }
    run_gpgv(
        keyring,
        &[
            signature.to_string_lossy().as_ref(),
            data.to_string_lossy().as_ref(),
        ],
    )
}

/// Verify a clearsigned document such as Debian's `InRelease`.
pub fn verify_inline(data: &Path, keyring: &Path) -> Result<()> {
    run_gpgv(keyring, &[data.to_string_lossy().as_ref()])
}

/// Verify an RPM package's OpenPGP signature with rpmkeys. Fedora does not
/// publish detached repomd signatures, so the package signature is the
/// cryptographic trust anchor there; a missing rpmkeys or key is an error.
pub fn verify_rpm_package(package: &Path, keyring: &Path) -> Result<()> {
    if !keyring.is_file() {
        return err(format!(
            "RPM keyring {} is missing; refusing unsigned package",
            keyring.display()
        ));
    }
    let db = std::env::temp_dir().join(format!(
        "forst-rpmdb-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&db)?;
    let cleanup = || {
        fs::remove_dir_all(&db).ok();
    };

    // rpmkeys creates the temporary rpmdb on import; no system rpmdb is
    // touched. It imports armored keys, while gpgv wants a binary keyring.
    // Export the same trust anchor in armored form for the import.
    // Export
    // the same trust anchor in armored form for the import.
    let armored = db.join("key.asc");
    let exported = Command::new("gpg")
        .args(["--no-default-keyring", "--keyring"])
        .arg(keyring)
        .arg("--batch")
        .arg("--armor")
        .arg("--export")
        .arg("--output")
        .arg(&armored)
        .output();
    match exported {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            cleanup();
            return err(format!(
                "cannot export RPM key: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Err(e) => {
            cleanup();
            return err(format!("RPM signature check needs gpg: {e}"));
        }
    }

    let imported = Command::new("rpmkeys")
        .arg("--dbpath")
        .arg(&db)
        .arg("--import")
        .arg(&armored)
        .output();
    match imported {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            cleanup();
            return err(format!(
                "rpm key import failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Err(e) => {
            cleanup();
            return err(format!("rpm signature check needs rpmkeys: {e}"));
        }
    }

    let checked = Command::new("rpmkeys")
        .arg("--dbpath")
        .arg(&db)
        .arg("--checksig")
        .arg(package)
        .output()
        .map_err(|e| Error::new(format!("cannot run rpmkeys: {e}")));
    let checked = match checked {
        Ok(out) => out,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    cleanup();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    if !checked.status.success() || text.contains("NOKEY") || text.contains("NOT OK") {
        return err(format!(
            "{}: RPM signature verification failed: {}",
            package.display(),
            text.trim()
        ));
    }
    Ok(())
}

/// Verify a file against a hex SHA256 that came from signed metadata.
/// An absent or malformed digest is an error, never a warning.
pub fn verify_sha256_file(path: &Path, expected: &str) -> Result<()> {
    let expected = expected.trim();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return err(format!(
            "{}: signed metadata carries no valid SHA256",
            path.display()
        ));
    }
    let actual = util::sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return err(format!(
            "{}: checksum mismatch against signed metadata\n  expected {}\n  actual   {}",
            path.display(),
            expected,
            actual
        ));
    }
    Ok(())
}

/// Find a file's digest in a Debian `Release`/`InRelease` SHA256 section.
///
/// Lines look like ` <hash> <size> <path>`, with paths relative to `dists/`.
/// Matching is exact after normalising a leading `./`.
pub fn debian_sha256(release: &str, wanted: &str) -> Option<String> {
    let wanted = wanted.trim_start_matches("./");
    let mut in_sha256 = false;
    for line in release.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("SHA256:") {
            in_sha256 = true;
            continue;
        }
        if !in_sha256 {
            continue;
        }
        if trimmed.is_empty() || trimmed.ends_with(':') && !trimmed.contains(' ') {
            in_sha256 = false;
            continue;
        }
        let mut fields = trimmed.split_whitespace();
        let Some(hash) = fields.next() else {
            continue;
        };
        let Some(_size) = fields.next() else {
            continue;
        };
        let Some(path) = fields.next() else {
            continue;
        };
        if path.trim_start_matches("./") == wanted {
            if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hash.to_ascii_lowercase());
            }
            return None;
        }
    }
    None
}

/// Extract the primary XML href and its SHA256 from signed `repomd.xml`.
/// The checksum must sit inside the same `<data type="primary">` element.
pub fn repomd_primary(repomd: &str) -> Option<(String, String)> {
    let start = repomd.find("<data type=\"primary\"")?;
    let tail = &repomd[start..];
    let end = tail.find("</data>").map(|e| e + 7).unwrap_or(tail.len());
    let block = &tail[..end];
    let href = block
        .find("<location ")
        .and_then(|i| block[i..].split_once("href=\""))
        .and_then(|(_, rest)| rest.split_once('"').map(|(v, _)| v.to_string()))?;
    let checksum_start = block.find("<checksum")?;
    let checksum = &block[checksum_start..];
    let open_end = checksum.find('>')?;
    let attrs = &checksum[..open_end];
    if !attrs.contains("type=\"sha256\"") {
        return None;
    }
    let value = checksum[open_end + 1..].split_once("</checksum>")?.0.trim();
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((href, value.to_ascii_lowercase()))
}

/// Read a keyring path, refusing an empty configuration rather than silently
/// falling back to an unrelated key.
pub fn keyring_or_default(configured: &str, defaults: &[&str]) -> Result<std::path::PathBuf> {
    if !configured.trim().is_empty() {
        return Ok(std::path::PathBuf::from(configured.trim()));
    }
    for candidate in defaults {
        let path = Path::new(candidate);
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
    }
    err(format!(
        "no signature keyring configured (tried: {}); refusing unsigned metadata",
        defaults.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn have_gpgv() -> bool {
        Command::new("gpgv")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn verifies_real_signature_and_rejects_tampering() {
        if !have_gpgv() {
            eprintln!("gpgv unavailable; signature test skipped");
            return;
        }
        let keyring = fixture("test-keyring.gpg");
        assert!(
            verify_detached(&fixture("release.txt"), &fixture("release.txt.sig"), &keyring)
                .is_ok(),
            "the fixture signature must verify"
        );
        let bad = verify_detached(
            &fixture("release-tampered.txt"),
            &fixture("release.txt.sig"),
            &keyring,
        );
        assert!(bad.is_err(), "tampered metadata must be rejected");
    }

    #[test]
    fn missing_signature_and_keyring_fail_closed() {
        if !have_gpgv() {
            return;
        }
        assert!(verify_detached(
            &fixture("release.txt"),
            &fixture("does-not-exist.sig"),
            &fixture("test-keyring.gpg")
        )
        .is_err());
        assert!(verify_detached(
            &fixture("release.txt"),
            &fixture("release.txt.sig"),
            &fixture("does-not-exist.gpg")
        )
        .is_err());
    }

    #[test]
    fn parses_debian_sha256_section() {
        let release = "Suite: stable\nSHA256:\n abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789 123 main/binary-amd64/Packages.xz\n";
        assert_eq!(
            debian_sha256(release, "main/binary-amd64/Packages.xz").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert!(debian_sha256(release, "main/binary-amd64/Packages.gz").is_none());
        assert!(debian_sha256("Suite: stable\n", "main/x").is_none());
    }

    #[test]
    fn parses_repomd_primary_and_requires_sha256() {
        let xml = r#"<repomd><data type="primary"><checksum type="sha256">aa</checksum><location href="repodata/primary.xml.zst"/></data><data type="filelists"><location href="repodata/filelists.xml"/></data></repomd>"#;
        assert!(repomd_primary(xml).is_none(), "short checksum must not pass");
        let good = format!(
            r#"<repomd><data type="primary"><checksum type="sha256">{}</checksum><location href="repodata/primary.xml.zst"/></data></repomd>"#,
            "ab".repeat(32)
        );
        let (href, sum) = repomd_primary(&good).expect("primary parsed");
        assert_eq!(href, "repodata/primary.xml.zst");
        assert_eq!(sum, "ab".repeat(32));
    }

    #[test]
    fn rpm_package_requires_a_keyring() {
        let missing = PathBuf::from("/nonexistent/forst-fedora-keyring.gpg");
        let err = verify_rpm_package(Path::new("package.rpm"), &missing)
            .expect_err("a missing RPM keyring must fail closed");
        assert!(err.to_string().contains("keyring"));
    }

    #[test]
    fn rejects_tampered_file_against_signed_checksum() {
        let dir = std::env::temp_dir().join(format!("forst-sign-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("package.pkg.tar.zst");
        fs::write(&path, b"the real package").unwrap();
        let good = util::sha256_file(&path).unwrap();
        assert!(verify_sha256_file(&path, &good).is_ok());
        fs::write(&path, b"a tampered package").unwrap();
        assert!(
            verify_sha256_file(&path, &good).is_err(),
            "a tampered package must be rejected"
        );
        assert!(verify_sha256_file(&path, "").is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
