//! Dependency resolution across codenames.
//!
//! Resolution is deliberately simple and predictable:
//!
//! * a dependency is resolved inside the codename of the package that wants it
//!   first, then base codenames first, then asset codenames;
//! * dependencies a base system cannot provide (sonames, file paths, virtual
//!   capabilities) never enter the graph -- they were dropped at index time;
//! * cycles are tolerated and broken by installation order, not by an error;
//! * anything that cannot be satisfied is reported. With `strict` it aborts.

use crate::config::{self, Format, Repo};
use crate::index::{Index, Package};
use crate::util::{err, warn, Result};
use std::collections::{BTreeMap, HashSet};

pub struct Catalog {
    pub repos: Vec<Repo>,
    pub indexes: BTreeMap<String, Index>,
}

impl Catalog {
    /// Load every cached index. With `allow_missing`, codenames that were never
    /// synced are skipped; otherwise they are an error pointing at `forst sync`.
    pub fn open(repos: Vec<Repo>, cache: &std::path::Path, allow_missing: bool) -> Result<Catalog> {
        let mut indexes = BTreeMap::new();
        let mut missing = Vec::new();
        for repo in repos.iter().filter(|r| r.enabled) {
            let path = repo.index_path(cache);
            if !path.exists() {
                missing.push(repo.codename.clone());
                continue;
            }
            match Index::load(&path) {
                Ok(index) => {
                    indexes.insert(repo.codename.clone(), index);
                }
                Err(e) => {
                    if allow_missing {
                        warn(&format!(
                            "ignoring unusable index for {} ({e}); run `forst sync {}`",
                            repo.codename, repo.codename
                        ));
                        missing.push(repo.codename.clone());
                    } else {
                        return err(format!(
                            "{} is unusable ({e}); run `forst sync {}`",
                            path.display(),
                            repo.codename
                        ));
                    }
                }
            }
        }
        if !missing.is_empty() {
            if allow_missing {
                for codename in &missing {
                    warn(&format!("codename {codename} has no index yet; skipped"));
                }
            } else if indexes.is_empty() {
                return err(format!(
                    "no repository indexes; run `forst sync` (missing: {})",
                    missing.join(", ")
                ));
            }
        }
        Ok(Catalog { repos, indexes })
    }

    /// Is this codename part of Forsteri's own base?
    pub fn is_base(&self, codename: &str) -> bool {
        self.repos
            .iter()
            .find(|r| r.codename == codename)
            .map(|r| r.is_base())
            .unwrap_or(true)
    }

    /// Look up a package in exactly one codename.
    pub fn find_in(&self, name: &str, codename: &str) -> Option<Package> {
        self.indexes.get(codename).and_then(|i| i.get(name)).cloned()
    }

    /// Look up a package in the base codenames only.
    pub fn find_base(&self, name: &str) -> Option<Package> {
        for repo in self.repos.iter().filter(|r| r.is_base()) {
            if let Some(index) = self.indexes.get(&repo.codename) {
                if let Some(pkg) = index.get(name) {
                    return Some(pkg.clone());
                }
            }
        }
        for repo in self.repos.iter().filter(|r| r.is_base()) {
            if let Some(index) = self.indexes.get(&repo.codename) {
                for pkg in &index.packages {
                    if pkg.provides.iter().any(|p| p == name) {
                        return Some(pkg.clone());
                    }
                }
            }
        }
        None
    }

    /// Members of a package group, from the first codename that has one.
    /// Groups are a base-system concept, so without an explicit codename only
    /// base codenames are consulted.
    pub fn group_members(
        &self,
        group: &str,
        codename: Option<&str>,
    ) -> Option<(String, Vec<Package>)> {
        let candidates: Vec<&Repo> = match codename {
            Some(c) => self.repos.iter().filter(|r| r.codename == c).collect(),
            None => config::default_order(&self.repos)
                .into_iter()
                .filter(|r| r.is_base())
                .collect(),
        };
        for repo in candidates {
            if let Some(index) = self.indexes.get(&repo.codename) {
                let members: Vec<Package> = index
                    .packages
                    .iter()
                    .filter(|p| p.groups.iter().any(|g| g == group))
                    .cloned()
                    .collect();
                if !members.is_empty() {
                    return Some((repo.codename.clone(), members));
                }
            }
        }
        None
    }

    /// Codenames that contain a package with this exact name.
    pub fn codenames_with(&self, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        for repo in config::default_order(&self.repos) {
            if let Some(index) = self.indexes.get(&repo.codename) {
                if index.get(name).is_some() {
                    out.push(repo.codename.clone());
                }
            }
        }
        out.dedup();
        out
    }

    pub fn find(&self, name: &str, prefer: Option<&str>) -> Option<Package> {
        if let Some(codename) = prefer {
            if let Some(index) = self.indexes.get(codename) {
                if let Some(pkg) = index.get(name) {
                    return Some(pkg.clone());
                }
            }
        }
        for repo in config::default_order(&self.repos) {
            if let Some(index) = self.indexes.get(&repo.codename) {
                if let Some(pkg) = index.get(name) {
                    return Some(pkg.clone());
                }
            }
        }
        // Virtual names (`sh`, `awk`, ...) advertised through provides.
        for repo in config::default_order(&self.repos) {
            if let Some(index) = self.indexes.get(&repo.codename) {
                for pkg in &index.packages {
                    if pkg.provides.iter().any(|p| p == name) {
                        return Some(pkg.clone());
                    }
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub codename: Option<String>,
    pub name: String,
    /// `@group` rather than a single package.
    pub group: bool,
}

/// `forst install codename:name`, `codename/name`, plain `name`, or `@group`
/// (`codename:@group` to pin the codename).
pub fn parse_spec(raw: &str, repos: &[Repo]) -> Spec {
    let raw = raw.trim();
    let mut group = false;
    let mut body = raw;
    for sep in [':', '/'] {
        if let Some((left, right)) = raw.split_once(sep) {
            if !right.is_empty() && config::find(repos, left).is_some() {
                body = right;
                let name = body.trim_start_matches('@');
                return Spec {
                    codename: Some(left.to_string()),
                    name: name.to_string(),
                    group: body.starts_with('@'),
                };
            }
        }
    }
    if let Some(stripped) = body.strip_prefix('@') {
        group = true;
        body = stripped;
    }
    Spec {
        codename: None,
        name: body.to_string(),
        group,
    }
}

#[derive(Debug)]
pub struct Plan {
    pub packages: Vec<Package>,
    pub missing: Vec<String>,
}

struct Resolver<'a> {
    catalog: &'a Catalog,
    installed: &'a HashSet<String>,
    satisfied: HashSet<String>,
    seen: HashSet<String>,
    in_progress: HashSet<String>,
    order: Vec<Package>,
    missing: Vec<String>,
}

impl<'a> Resolver<'a> {
    /// Add a package and everything it needs, dependencies first.
    ///
    /// A package from a base codename only ever resolves its dependencies
    /// inside the base: `colony:tree` must never quietly pull `tundra:libc6`.
    /// A package from an asset codename may fall back to the base, and then to
    /// other asset codenames, because its deps are upstream names.
    fn schedule(&mut self, pkg: Package, depth: usize) -> Result<()> {
        if depth > 64 {
            return err(format!("dependency chain deeper than 64 at {}", pkg.spec()));
        }
        let key = pkg.spec();
        if self.seen.contains(&key) || self.in_progress.contains(&key) {
            return Ok(());
        }
        self.in_progress.insert(key.clone());
        let is_base = self.catalog.is_base(&pkg.codename);
        let deps = pkg.depends.clone();
        for dep in deps {
            if self.satisfied.contains(&dep) || self.installed.contains(&dep) {
                continue;
            }
            // An asset package's system-level requirements (libc6, bash, ...)
            // are already provided by Forsteri's base. Pulling the foreign copy
            // would overwrite the base, so the dependency is considered met.
            if !is_base && is_system_package(&dep) {
                continue;
            }
            let found = if is_base {
                self.catalog.find_base(&dep)
            } else {
                self.catalog.find(&dep, Some(&pkg.codename))
            };
            match found {
                Some(dep_pkg) => self.schedule(dep_pkg, depth + 1)?,
                None => {
                    if !self.missing.contains(&dep) {
                        self.missing.push(dep);
                    }
                }
            }
        }
        self.in_progress.remove(&key);
        self.seen.insert(key);
        for provided in &pkg.provides {
            self.satisfied.insert(provided.clone());
        }
        self.satisfied.insert(pkg.name.clone());
        self.order.push(pkg);
        Ok(())
    }
}

pub fn resolve(
    catalog: &Catalog,
    specs: &[Spec],
    installed: &HashSet<String>,
    strict: bool,
) -> Result<Plan> {
    let mut resolver = Resolver {
        catalog,
        installed,
        satisfied: installed.clone(),
        seen: HashSet::new(),
        in_progress: HashSet::new(),
        order: Vec::new(),
        missing: Vec::new(),
    };

    for spec in specs {
        if spec.group {
            match resolver
                .catalog
                .group_members(&spec.name, spec.codename.as_deref())
            {
                Some((_codename, members)) => {
                    for member in members {
                        resolver.schedule(member, 0)?;
                    }
                }
                None => {
                    return err(format!(
                        "no package group named @{} in any synced codename",
                        spec.name
                    ))
                }
            }
            continue;
        }
        let found = match &spec.codename {
            Some(codename) => match resolver.catalog.find_in(&spec.name, codename) {
                Some(pkg) => Some(pkg),
                None => {
                    let elsewhere = resolver.catalog.codenames_with(&spec.name);
                    let hint = if elsewhere.is_empty() {
                        String::new()
                    } else {
                        format!(" (it exists in: {})", elsewhere.join(", "))
                    };
                    return err(format!(
                        "no package {:?} in codename {codename}{hint}",
                        spec.name
                    ));
                }
            },
            None => resolver.catalog.find(&spec.name, None),
        };
        match found {
            Some(pkg) => resolver.schedule(pkg, 0)?,
            None => {
                return err(format!(
                    "no package named {:?} in any synced codename",
                    spec.name
                ))
            }
        }
    }

    if strict && !resolver.missing.is_empty() {
        return err(format!(
            "unresolved dependencies: {}",
            resolver.missing.join(", ")
        ));
    }

    Ok(Plan {
        packages: resolver.order,
        missing: resolver.missing,
    })
}

/// Packages a foreign codename must never replace: they are the base system.
const SYSTEM_PACKAGES: &[&str] = &[
    "glibc",
    "libc6",
    "libc-bin",
    "libgcc",
    "bash",
    "coreutils",
    "systemd",
    "systemd-sysv",
    "dbus",
    "dbus-daemon",
    "dbus-broker",
    "pacman",
    "apt",
    "dpkg",
    "rpm",
    "dnf",
    "kernel",
    "linux",
    "linux-lts",
    "linux-firmware",
    "mkinitcpio",
    "grub",
    "sudo",
    "polkit",
    "mesa",
    "wayland",
    "wayland-server",
];

/// Packages Forsteri's base always provides, no matter the codename a
/// dependent package came from.
pub fn is_system_package(name: &str) -> bool {
    SYSTEM_PACKAGES.contains(&name)
}

/// Why installing `pkg` from its codename is refused, if it is.
pub fn foreign_guard(pkg: &Package, catalog: &Catalog, allow: bool) -> Option<String> {
    let format = catalog
        .repos
        .iter()
        .find(|r| r.codename == pkg.codename)
        .map(|r| r.format)
        .unwrap_or(Format::Pacman);
    if format == Format::Pacman || allow {
        return None;
    }
    if SYSTEM_PACKAGES.contains(&pkg.name.as_str()) {
        return Some(format!(
            "{codename}:{name} is a base-system package; installing a {format} system package over Forsteri's base would break the system. Use `--allow-foreign-system` if you really mean it.",
            codename = pkg.codename,
            name = pkg.name,
            format = format.as_str()
        ));
    }
    None
}

pub fn installed_names(metas: &[crate::install::Meta]) -> HashSet<String> {
    let mut set = HashSet::new();
    for meta in metas {
        set.insert(meta.name.clone());
        for p in &meta.provides {
            set.insert(p.clone());
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(codename: &str, name: &str, version: &str, depends: &[&str]) -> Package {
        Package {
            name: name.into(),
            version: version.into(),
            arch: "x86_64".into(),
            filename: format!("{name}.pkg"),
            size: 1,
            sha256: String::new(),
            provides: Vec::new(),
            depends: depends.iter().map(|d| d.to_string()).collect(),
            groups: Vec::new(),
            description: String::new(),
            codename: codename.into(),
        }
    }

    fn grouped(codename: &str, name: &str, group: &str) -> Package {
        let mut pkg = package(codename, name, "1-1", &[]);
        pkg.groups = vec![group.to_string()];
        pkg
    }

    fn catalog() -> Catalog {
        let repos = vec![
            Repo::pacman("colony", "https://c", "core", "x86_64", "core"),
            Repo::pacman("ice", "https://i", "extra", "x86_64", "extra"),
            Repo::debian("tundra", "http://d", "bookworm", "main", "amd64", "deb"),
        ];
        let mut indexes = BTreeMap::new();
        indexes.insert(
            "colony".to_string(),
            Index::new(
                "colony",
                vec![
                    package("colony", "app", "1-1", &["glibc", "libssl"]),
                    package("colony", "libssl", "3-1", &["glibc"]),
                ],
            ),
        );
        indexes.insert(
            "ice".to_string(),
            Index::new(
                "ice",
                vec![
                    package("ice", "tree", "2-1", &["glibc"]),
                    grouped("ice", "gnome-shell", "gnome"),
                    grouped("ice", "gnome-terminal", "gnome"),
                ],
            ),
        );
        indexes.insert(
            "tundra".to_string(),
            Index::new(
                "tundra",
                vec![
                    package("tundra", "glibc", "2.36", &[]),
                    package("tundra", "hello", "2.10", &["libc6"]),
                    package("tundra", "tree", "2.1", &["libc6"]),
                ],
            ),
        );
        Catalog { repos, indexes }
    }

    fn installed(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pinned_codename_does_not_fall_back() {
        let catalog = catalog();
        assert!(catalog.find_in("tree", "colony").is_none());
        assert_eq!(catalog.codenames_with("tree"), vec!["ice", "tundra"]);
        let spec = Spec {
            codename: Some("colony".into()),
            name: "tree".into(),
            group: false,
        };
        let err = resolve(&catalog, &[spec], &installed(&[]), false).unwrap_err();
        assert!(err.to_string().contains("it exists in: ice, tundra"), "{err}");
    }

    #[test]
    fn base_packages_never_pull_foreign_dependencies() {
        let catalog = catalog();
        let spec = Spec {
            codename: Some("colony".into()),
            name: "app".into(),
            group: false,
        };
        let plan = resolve(&catalog, &[spec], &installed(&[]), false).unwrap();
        assert!(plan.packages.iter().all(|p| p.codename == "colony"));
        assert_eq!(plan.missing, vec!["glibc"], "glibc is not in the base, so it is reported, not taken from Debian");
        // dependency-first order
        let names: Vec<&str> = plan.packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["libssl", "app"]);
    }

    #[test]
    fn asset_packages_treat_system_requirements_as_provided_by_the_base() {
        let catalog = catalog();
        let spec = Spec {
            codename: Some("tundra".into()),
            name: "hello".into(),
            group: false,
        };
        // hello depends on libc6, which the base provides: no Debian libc6 is
        // planned, and nothing is reported missing.
        let plan = resolve(&catalog, &[spec], &installed(&[]), true).unwrap();
        assert_eq!(plan.packages.len(), 1);
        assert_eq!(plan.packages[0].codename, "tundra");
        assert!(plan.missing.is_empty());
    }


    #[test]
    fn asset_packages_may_still_pull_other_assets() {
        let catalog = catalog();
        let spec = Spec {
            codename: Some("tundra".into()),
            name: "tree".into(),
            group: false,
        };
        let plan = resolve(&catalog, &[spec], &installed(&[]), false).unwrap();
        assert_eq!(plan.packages.len(), 1);
        assert_eq!(plan.packages[0].name, "tree");
    }

    #[test]
    fn groups_expand_from_the_first_matching_base_codename() {
        let catalog = catalog();
        let spec = Spec {
            codename: None,
            name: "gnome".into(),
            group: true,
        };
        let plan = resolve(&catalog, &[spec], &installed(&[]), false).unwrap();
        assert_eq!(plan.packages.len(), 2);
        assert!(plan.packages.iter().all(|p| p.codename == "ice"));
        assert!(plan.packages.iter().any(|p| p.name == "gnome-shell"));
    }

    #[test]
    fn unknown_group_is_an_error() {
        let catalog = catalog();
        let spec = Spec {
            codename: None,
            name: "kde".into(),
            group: true,
        };
        let err = resolve(&catalog, &[spec], &installed(&[]), false).unwrap_err();
        assert!(err.to_string().contains("@kde"), "{err}");
    }

    #[test]
    fn already_installed_dependencies_are_skipped() {
        let catalog = catalog();
        let spec = Spec {
            codename: Some("ice".into()),
            name: "tree".into(),
            group: false,
        };
        let plan = resolve(&catalog, &[spec], &installed(&["glibc"]), false).unwrap();
        assert_eq!(plan.packages.len(), 1);
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn group_specs_parse() {
        let repos = catalog().repos;
        let spec = parse_spec("@gnome", &repos);
        assert!(spec.group);
        assert_eq!(spec.name, "gnome");
        let pinned = parse_spec("ice:@gnome", &repos);
        assert!(pinned.group);
        assert_eq!(pinned.codename.as_deref(), Some("ice"));
        assert_eq!(pinned.name, "gnome");
    }

    #[test]
    fn strict_mode_rejects_unresolved_dependencies() {
        let catalog = catalog();
        let spec = Spec {
            codename: Some("colony".into()),
            name: "app".into(),
            group: false,
        };
        assert!(resolve(&catalog, &[spec], &installed(&[]), true).is_err());
    }

    #[test]
    fn guard_blocks_foreign_system_packages() {
        let catalog = catalog();
        let glibc = catalog.find_in("glibc", "tundra").unwrap();
        assert!(foreign_guard(&glibc, &catalog, false).is_some());
        assert!(foreign_guard(&glibc, &catalog, true).is_none());
        let hello = catalog.find_in("hello", "tundra").unwrap();
        assert!(foreign_guard(&hello, &catalog, false).is_none());
        let app = catalog.find_in("app", "colony").unwrap();
        assert!(foreign_guard(&app, &catalog, false).is_none());
    }

    #[test]
    fn parses_codename_specs() {
        let repos = catalog().repos;
        let spec = parse_spec("tundra:htop", &repos);
        assert_eq!(spec.codename.as_deref(), Some("tundra"));
        assert_eq!(spec.name, "htop");
        assert_eq!(parse_spec("tundra/htop", &repos).codename.as_deref(), Some("tundra"));
        let plain = parse_spec("htop", &repos);
        assert!(plain.codename.is_none());
        assert!(!plain.group);
        // unknown codename stays part of the name
        assert!(parse_spec("nope:htop", &repos).codename.is_none());
    }
}
