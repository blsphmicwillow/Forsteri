//! Repository registry: codenames and how each one is fetched.
//!
//! A "codename" is the only thing a user has to remember. It names a remote
//! collection of packages, its on-disk format and its mirror:
//!
//! ```text
//! colony  Arch core (pacman format, base system)
//! ice     Arch extra (pacman format)
//! nest    Forsteri's own repo (pacman format)
//! tundra  Debian stable main (deb format)
//! aurora  Fedora Everything (rpm format)
//! ```
//!
//! Codenames whose format is `pacman` are *base* repos: they share Forsteri's
//! package database and are safe to install into. `deb` and `rpm` codenames are
//! *asset* repos: they are a source of files, not of a second libc.

use crate::util::{err, Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pacman,
    Debian,
    Rpm,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Pacman => "pacman",
            Format::Debian => "deb",
            Format::Rpm => "rpm",
        }
    }

    pub fn parse(s: &str) -> Option<Format> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pacman" | "arch" => Some(Format::Pacman),
            "deb" | "debian" | "apt" => Some(Format::Debian),
            "rpm" | "fedora" | "dnf" => Some(Format::Rpm),
            _ => None,
        }
    }

}

#[derive(Debug, Clone)]
pub struct Repo {
    pub codename: String,
    pub format: Format,
    /// Mirror root, without a trailing slash.
    pub url: String,
    pub arch: String,
    /// pacman: database name (`core`, `extra`, ...).
    pub db: String,
    /// deb: suite (`bookworm`, `trixie`, ...).
    pub suite: String,
    /// deb: component (`main`, `contrib`, ...).
    pub component: String,
    pub enabled: bool,
    pub description: String,
    /// OpenPGP keyring used for this codename's signed metadata. Empty means
    /// "use the format's shipped keyring"; an explicit path is authoritative.
    pub keyring: String,
}

impl Repo {
    #[cfg(test)]
    pub fn pacman(codename: &str, url: &str, db: &str, arch: &str, description: &str) -> Repo {
        Repo {
            codename: codename.to_string(),
            format: Format::Pacman,
            url: url.trim_end_matches('/').to_string(),
            arch: arch.to_string(),
            db: db.to_string(),
            suite: String::new(),
            component: String::new(),
            enabled: true,
            description: description.to_string(),
            keyring: String::new(),
        }
    }

    pub fn debian(
        codename: &str,
        url: &str,
        suite: &str,
        component: &str,
        arch: &str,
        description: &str,
    ) -> Repo {
        Repo {
            codename: codename.to_string(),
            format: Format::Debian,
            url: url.trim_end_matches('/').to_string(),
            arch: arch.to_string(),
            db: String::new(),
            suite: suite.to_string(),
            component: component.to_string(),
            enabled: true,
            description: description.to_string(),
            keyring: String::new(),
        }
    }

    pub fn rpm(codename: &str, url: &str, arch: &str, description: &str) -> Repo {
        Repo {
            codename: codename.to_string(),
            format: Format::Rpm,
            url: url.trim_end_matches('/').to_string(),
            arch: arch.to_string(),
            db: String::new(),
            suite: String::new(),
            component: String::new(),
            enabled: true,
            description: description.to_string(),
            keyring: String::new(),
        }
    }

    /// Arch does not sign repository databases (pacman trusts package
    /// signatures), and Fedora does not sign repomd either. Both formats are
    /// verified at the package boundary. Debian instead signs its metadata
    /// and binds the Packages checksum, so it does not need a per-package
    /// signature.
    pub fn requires_package_signature(&self) -> bool {
        matches!(self.format, Format::Pacman | Format::Rpm)
    }

    /// True for codenames that belong to Forsteri's own package base.
    pub fn is_base(&self) -> bool {
        self.format == Format::Pacman
    }

    /// URL of the repository index.
    pub fn index_url(&self) -> String {
        match self.format {
            Format::Pacman => format!("{}/{}.db", self.url, self.db),
            Format::Debian => format!(
                "{}/dists/{}/{}/binary-{}/Packages.xz",
                self.url, self.suite, self.component, self.arch
            ),
            Format::Rpm => format!("{}/repodata/repomd.xml", self.url),
        }
    }

    /// URL of the detached signature over `index_url` (pacman/rpm), or of the
    /// clearsigned Debian `InRelease` document.
    pub fn signature_url(&self) -> String {
        match self.format {
            Format::Pacman => format!("{}.sig", self.index_url()),
            Format::Debian => format!("{}/dists/{}/InRelease", self.url, self.suite),
            Format::Rpm => format!("{}/repodata/repomd.xml.asc", self.url),
        }
    }

    /// Keyring for signature checks, refusing to fall through to an unrelated
    /// key when the configured path is bad.
    pub fn keyring_path(&self) -> Result<PathBuf> {
        let defaults: &[&str] = match self.format {
            Format::Pacman => &[
                "/etc/pacman.d/gnupg/pubring.gpg",
                "/usr/share/forsteri/keyrings/arch.gpg",
            ],
            Format::Debian => &[
                "/usr/share/forsteri/keyrings/debian.gpg",
                "/usr/share/keyrings/debian-archive-keyring.gpg",
            ],
            Format::Rpm => &[
                "/usr/share/forsteri/keyrings/fedora.gpg",
                "/etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-43-primary",
            ],
        };
        crate::sign::keyring_or_default(&self.keyring, defaults)
    }

    pub fn package_url(&self, filename: &str) -> String {
        format!("{}/{}", self.url, filename.trim_start_matches('/'))
    }

    pub fn index_path(&self, cache: &Path) -> PathBuf {
        cache.join("index").join(format!("{}.idx", self.codename))
    }

    pub fn summary(&self) -> String {
        match self.format {
            Format::Pacman => format!("arch {} ({})", self.db, self.arch),
            Format::Debian => format!("debian {}/{}/{}", self.suite, self.component, self.arch),
            Format::Rpm => format!("fedora ({})", self.arch),
        }
    }
}

pub const DEFAULT_CONF: &str = r#"# /etc/forst/repos.conf -- Forsteri repository codenames.
#
# forst <command> codename:package    (or just "package" for base order)
#
# format = pacman   base system, shares /var/lib/forst with Forsteri
# format = deb      Debian assets  (extracted, recorded, not linked to a libc)
# format = rpm      Fedora assets  (extracted, recorded, not linked to a libc)

[colony]
format      = pacman
url         = https://geo.mirror.pkgbuild.com/core/os/x86_64
db          = core
arch        = x86_64
enabled     = yes
keyring     = /etc/pacman.d/gnupg/pubring.gpg
description = Arch core -- the Forsteri base system

[ice]
format      = pacman
url         = https://geo.mirror.pkgbuild.com/extra/os/x86_64
db          = extra
arch        = x86_64
enabled     = yes
keyring     = /etc/pacman.d/gnupg/pubring.gpg
description = Arch extra -- desktops, apps, fonts

[nest]
format      = pacman
url         = file:///srv/forsteri/repo/x86_64
db          = nest
arch        = x86_64
enabled     = yes
keyring     = /etc/pacman.d/gnupg/pubring.gpg
description = Forsteri's own packages (branding, themes, quickshell, forst)

[tundra]
format      = deb
url         = http://deb.debian.org/debian
suite       = bookworm
component   = main
arch        = amd64
enabled     = yes
keyring     = /usr/share/forsteri/keyrings/debian.gpg
description = Debian stable main -- .deb assets

[aurora]
format      = rpm
url         = https://download.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os
arch        = x86_64
enabled     = yes
keyring     = /usr/share/forsteri/keyrings/fedora.gpg
description = Fedora 43 Everything -- .rpm assets
"#;

/// Read `/etc/forst/repos.conf`-style INI. A missing file means the built-in
/// defaults, which are exactly the contents of `DEFAULT_CONF`.
pub fn load(path: &Path) -> Result<Vec<Repo>> {
    if !path.exists() {
        return parse_conf(DEFAULT_CONF);
    }
    let text = fs::read_to_string(path)
        .map_err(|e| Error::new(format!("cannot read {}: {e}", path.display())))?;
    parse_conf(&text)
}

pub fn parse_conf(text: &str) -> Result<Vec<Repo>> {
    let mut repos: Vec<Repo> = Vec::new();
    let mut current: Option<Repo> = None;

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some(repo) = current.take() {
                repos.push(repo);
            }
            current = Some(Repo {
                codename: name.trim().to_string(),
                format: Format::Pacman,
                url: String::new(),
                arch: "x86_64".to_string(),
                db: String::new(),
                suite: String::new(),
                component: "main".to_string(),
                enabled: true,
                description: String::new(),
                keyring: String::new(),
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return err(format!(
                "{}:{}: expected key = value",
                path_label(),
                lineno + 1
            ));
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        let Some(repo) = current.as_mut() else {
            return err(format!(
                "{}:{}: setting outside of a [codename] section",
                path_label(),
                lineno + 1
            ));
        };
        match key.as_str() {
            "format" => {
                repo.format = Format::parse(&value).ok_or_else(|| {
                    Error::new(format!(
                        "{}:{}: unknown format {value:?} (pacman, deb, rpm)",
                        path_label(),
                        lineno + 1
                    ))
                })?;
            }
            "url" | "mirror" => repo.url = value.trim_end_matches('/').to_string(),
            "arch" => repo.arch = value,
            "db" | "database" => repo.db = value,
            "suite" | "release" => repo.suite = value,
            "component" | "components" => repo.component = value,
            "enabled" => repo.enabled = matches!(value.as_str(), "yes" | "true" | "1" | "on"),
            "description" | "desc" => repo.description = value,
            "keyring" => repo.keyring = value,
            other => crate::util::warn(&format!("repos.conf: ignoring key {other:?}")),
        }
    }
    if let Some(repo) = current.take() {
        repos.push(repo);
    }

    for repo in &mut repos {
        if repo.url.is_empty() {
            return err(format!("codename {:?} has no url", repo.codename));
        }
        if repo.format == Format::Pacman && repo.db.is_empty() {
            repo.db = repo.codename.clone();
        }
        if repo.format == Format::Debian && repo.suite.is_empty() {
            return err(format!("codename {:?} has no suite", repo.codename));
        }
    }
    if repos.is_empty() {
        return err("repos.conf defines no codenames");
    }
    Ok(repos)
}

fn path_label() -> &'static str {
    "repos.conf"
}

/// Find a codename, case-insensitively.
pub fn find<'a>(repos: &'a [Repo], codename: &str) -> Option<&'a Repo> {
    repos
        .iter()
        .find(|r| r.codename.eq_ignore_ascii_case(codename.trim()))
}

/// The order used when a spec has no codename: base repos first, then assets.
pub fn default_order(repos: &[Repo]) -> Vec<&Repo> {
    let mut order: Vec<&Repo> = repos.iter().filter(|r| r.is_base()).collect();
    order.extend(repos.iter().filter(|r| !r.is_base()));
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_conf() {
        let repos = parse_conf(DEFAULT_CONF).expect("conf parses");
        assert_eq!(repos.len(), 5);
        let colony = find(&repos, "COLONY").unwrap();
        assert_eq!(colony.format, Format::Pacman);
        assert!(colony.is_base());
        assert_eq!(
            colony.index_url(),
            "https://geo.mirror.pkgbuild.com/core/os/x86_64/core.db"
        );
        let tundra = find(&repos, "tundra").unwrap();
        assert!(!tundra.is_base());
        assert_eq!(
            tundra.index_url(),
            "http://deb.debian.org/debian/dists/bookworm/main/binary-amd64/Packages.xz"
        );
        let aurora = find(&repos, "aurora").unwrap();
        assert_eq!(colony.keyring, "/etc/pacman.d/gnupg/pubring.gpg");
        assert_eq!(
            tundra.signature_url(),
            "http://deb.debian.org/debian/dists/bookworm/InRelease"
        );
        assert_eq!(
            aurora.signature_url(),
            "https://download.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/repodata/repomd.xml.asc"
        );
        assert_eq!(
            aurora.index_url(),
            "https://download.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/repodata/repomd.xml"
        );
        assert_eq!(default_order(&repos).len(), 5);
        assert_eq!(default_order(&repos)[0].codename, "colony");
    }

    #[test]
    fn rejects_unknown_format() {
        assert!(parse_conf("[nope]\nformat = rpm2\nurl = http://x\n").is_err());
    }
}
