//! Repository indexes.
//!
//! Every codename is normalised into one flat record set, cached under
//! `/var/cache/forst/index/<codename>.idx`, so that resolution, search and
//! install never need to know which upstream format a package came from.
//!
//! Index line format (tab separated):
//!
//! ```text
//! name  version  arch  filename  size  sha256  provides  depends  description
//! ```
//!
//! `provides` and `depends` are space separated *plain names*; version
//! constraints, alternatives and sonames are resolved away at parse time (forst
//! resolves package names, not shared objects -- see docs/codenames.md).

use crate::config::{Format, Repo};
use crate::util::{err, decompress_bytes, Error, Result};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
    pub provides: Vec<String>,
    pub depends: Vec<String>,
    /// Package groups (`@gnome`, `@plasma`, ...). Only useful for base
    /// codenames; upstream asset repos rarely carry meaningful groups.
    pub groups: Vec<String>,
    pub description: String,
    pub codename: String,
}

impl Package {
    pub fn spec(&self) -> String {
        format!("{}:{}", self.codename, self.name)
    }
}

fn clean(value: &str) -> String {
    value
        .replace(['\t', '\n', '\r'], " ")
        .trim()
        .to_string()
}

/// Reduce an upstream dependency string to a bare package name, or `None` when
/// it is a file/soname/virtual dependency that forst deliberately ignores.
pub fn normalize_dep(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Debian alternatives: "libc6-dev | libc-dev" -> take the first.
    let first = raw.split('|').next().unwrap_or(raw).trim();
    // Take the leading token, which drops Debian's "libc6 (>= 2.34)" form.
    let first = first.split_whitespace().next().unwrap_or(first);
    // rpm sonames and virtual capabilities: libm.so.6()(64bit), rpmlib(...),
    // python(abi), application(0ad.desktop), pkgconfig(foo).
    if first.contains('(') || first.contains(".so") {
        return None;
    }
    // File dependencies (/usr/bin/sh on Fedora, /bin/sh on Debian).
    if first.starts_with('/') {
        return None;
    }
    // Strip version constraints: "glibc>=2.38".
    let name = first.split(['<', '>', '=']).next().unwrap_or(first).trim();
    // Drop a repository qualifier: "acl:any" -> "acl".
    let name = name.split(':').next().unwrap_or(name).trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

fn normalize_list(items: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if let Some(name) = normalize_dep(item) {
            if !out.contains(&name) && name.len() < 200 {
                out.push(name);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// cache file
// ---------------------------------------------------------------------------

pub struct Index {
    pub codename: String,
    pub packages: Vec<Package>,
    lookup: HashMap<String, usize>,
}

impl Index {
    pub fn new(codename: &str, packages: Vec<Package>) -> Index {
        let mut lookup = HashMap::new();
        for (i, p) in packages.iter().enumerate() {
            lookup.entry(p.name.clone()).or_insert(i);
        }
        Index {
            codename: codename.to_string(),
            packages,
            lookup,
        }
    }

    pub fn get(&self, name: &str) -> Option<&Package> {
        self.lookup.get(name).map(|&i| &self.packages[i])
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = String::new();
        out.push_str(&format!("#forst2\t{}\t{}\n", self.codename, self.packages.len()));
        for p in &self.packages {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                p.name,
                p.version,
                p.arch,
                p.filename,
                p.size,
                p.sha256,
                p.provides.join(" "),
                p.depends.join(" "),
                p.groups.join(" "),
                clean(&p.description)
            ));
        }
        fs::write(path, out)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Index> {
        let text = fs::read_to_string(path)
            .map_err(|e| Error::new(format!("cannot read index {}: {e}", path.display())))?;
        let mut lines = text.lines();
        let header = lines.next().unwrap_or("");
        let mut parts = header.split('\t');
        if parts.next() != Some("#forst2") {
            return err(format!("{}: not a forst index", path.display()));
        }
        let codename = parts.next().unwrap_or("").to_string();
        let mut packages = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 10 {
                continue;
            }
            packages.push(Package {
                name: f[0].to_string(),
                version: f[1].to_string(),
                arch: f[2].to_string(),
                filename: f[3].to_string(),
                size: f[4].parse().unwrap_or(0),
                sha256: f[5].to_string(),
                provides: f[6].split_whitespace().map(str::to_string).collect(),
                depends: f[7].split_whitespace().map(str::to_string).collect(),
                groups: f[8].split_whitespace().map(str::to_string).collect(),
                description: f[9].to_string(),
                codename: codename.clone(),
            });
        }
        Ok(Index::new(&codename, packages))
    }
}

// ---------------------------------------------------------------------------
// parsing, dispatched by format
// ---------------------------------------------------------------------------

/// Parse a raw (possibly compressed) index into packages.
pub fn parse(repo: &Repo, raw: &[u8]) -> Result<Vec<Package>> {
    match repo.format {
        Format::Pacman => {
            let data = decompress_bytes(raw)?;
            parse_pacman_db(&data, repo)
        }
        Format::Debian => {
            let data = decompress_bytes(raw)?;
            let text = String::from_utf8_lossy(&data).into_owned();
            Ok(parse_debian_packages(&text, repo))
        }
        Format::Rpm => {
            let text = String::from_utf8_lossy(raw).into_owned();
            Ok(parse_rpm_primary(&text, repo))
        }
    }
}

// --- pacman: tar of %FIELD% blocks -----------------------------------------

fn desc_fields(text: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let mut key: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('%') && line.ends_with('%') && line.len() > 2 {
            let name = line[1..line.len() - 1].to_string();
            map.entry(name.clone()).or_default();
            key = Some(name);
        } else if let Some(k) = &key {
            if !line.trim().is_empty() {
                map.entry(k.clone()).or_default().push(line.to_string());
            }
        }
    }
    map
}

fn field_one(map: &HashMap<String, Vec<String>>, key: &str) -> String {
    map.get(key)
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_default()
}

fn field_all(map: &HashMap<String, Vec<String>>, key: &str) -> Vec<String> {
    map.get(key).cloned().unwrap_or_default()
}

pub fn parse_pacman_db(db: &[u8], repo: &Repo) -> Result<Vec<Package>> {
    let mut packages = Vec::new();
    let mut reader = Cursor::new(db);
    crate::util::tar_foreach(&mut reader, &mut |entry| {
        if !entry.name.ends_with("/desc") {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&entry.data);
        let map = desc_fields(&text);
        let name = field_one(&map, "NAME");
        let filename = field_one(&map, "FILENAME");
        if name.is_empty() || filename.is_empty() {
            return Ok(());
        }
        let arch = field_one(&map, "ARCH");
        if !arch.is_empty() && arch != repo.arch && arch != "any" {
            return Ok(());
        }
        let depends = field_all(&map, "DEPENDS");
        let provides = field_all(&map, "PROVIDES");
        let groups = field_all(&map, "GROUPS");
        packages.push(Package {
            name,
            version: field_one(&map, "VERSION"),
            arch,
            filename,
            size: field_one(&map, "CSIZE").parse().unwrap_or(0),
            sha256: field_one(&map, "SHA256SUM"),
            provides: normalize_list(&provides.iter().map(String::as_str).collect::<Vec<_>>()),
            depends: normalize_list(&depends.iter().map(String::as_str).collect::<Vec<_>>()),
            groups: groups
                .iter()
                .map(|g| clean(g))
                .filter(|g| !g.is_empty())
                .collect(),
            description: clean(&field_one(&map, "DESC")),
            codename: repo.codename.clone(),
        });
        Ok(())
    })?;
    Ok(packages)
}

// --- debian: RFC822 stanzas -------------------------------------------------

pub fn parse_debian_packages(text: &str, repo: &Repo) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut stanza: HashMap<String, String> = HashMap::new();
    let mut last_key = String::new();

    let flush = |stanza: &mut HashMap<String, String>, packages: &mut Vec<Package>| {
        let get = |k: &str| stanza.get(k).cloned().unwrap_or_default();
        let name = get("Package");
        let filename = get("Filename");
        let arch = get("Architecture");
        if !name.is_empty()
            && !filename.is_empty()
            && (arch == repo.arch || arch == "all" || arch.is_empty())
        {
            let depends = get("Depends");
            let depends: Vec<&str> = depends.split(',').collect();
            let provides = get("Provides");
            let provides: Vec<&str> = provides.split(',').collect();
            let description = get("Description");
            let short = description.lines().next().unwrap_or("").to_string();
            packages.push(Package {
                name,
                version: get("Version"),
                arch,
                filename,
                size: get("Size").parse().unwrap_or(0),
                sha256: {
                    let s = get("SHA256");
                    if s.is_empty() {
                        get("SHA1")
                    } else {
                        s
                    }
                },
                provides: normalize_list(&provides),
                depends: normalize_list(&depends),
                groups: {
                    let section = get("Section");
                    section
                        .split('/')
                        .last()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .into_iter()
                        .collect()
                },
                description: clean(&short),
                codename: repo.codename.clone(),
            });
        }
        stanza.clear();
    };

    for line in text.lines() {
        if line.trim().is_empty() {
            if !stanza.is_empty() {
                flush(&mut stanza, &mut packages);
            }
            last_key.clear();
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line: keep only the first description line.
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            last_key = k.trim().to_string();
            stanza.insert(last_key.clone(), v.trim().to_string());
        }
        let _ = &last_key;
    }
    if !stanza.is_empty() {
        flush(&mut stanza, &mut packages);
    }
    packages
}

// --- rpm: primary.xml -------------------------------------------------------

fn between<'a>(hay: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let s = hay.find(start)? + start.len();
    let e = hay[s..].find(end)? + s;
    Some(&hay[s..e])
}

fn tag_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    between(block, &open, &format!("</{tag}>"))
}

/// Fedora writes `<checksum type="sha256" pkgid="YES">...`, so the plain
/// `tag_text("checksum")` form silently returned nothing. Require sha256;
/// do not fall back to MD5/SHA1 values as if they were SHA256.
fn sha256_checksum(block: &str) -> String {
    let Some(start) = block.find("<checksum") else {
        return String::new();
    };
    let tail = &block[start..];
    let Some(open_end) = tail.find('>') else {
        return String::new();
    };
    let attrs = &tail[..open_end];
    if !attrs.contains("type=\"sha256\"") {
        return String::new();
    }
    tail[open_end + 1..]
        .split_once("</checksum>")
        .map(|(value, _)| value.trim().to_string())
        .unwrap_or_default()
}

fn attr<'a>(block: &'a str, tag: &str, attribute: &str) -> Option<&'a str> {
    let open = format!("<{tag} ");
    let element = between(block, &open, ">")?;
    let needle = format!("{attribute}=\"");
    let v = between(element, &needle, "\"")?;
    Some(v)
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", " ")
        .replace("&amp;", "&")
}

fn rpm_entries(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(start) = rest.find("<rpm:entry ") {
        let tail = &rest[start..];
        let Some(element) = tail.split_once('>').map(|x| x.0) else {
            break;
        };
        if let Some(name) = between(element, "name=\"", "\"") {
            out.push(name.to_string());
        }
        rest = &tail[element.len()..];
    }
    out
}

pub fn parse_rpm_primary(xml: &str, repo: &Repo) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<package ") {
        let block = &rest[start..];
        let end = block.find("</package>").map(|e| e + 10).unwrap_or(block.len());
        let block = &block[..end];
        rest = &rest[start + end..];

        let Some(name) = tag_text(block, "name") else {
            continue;
        };
        let Some(href) = attr(block, "location", "href") else {
            continue;
        };
        let arch = tag_text(block, "arch").unwrap_or("");
        if !arch.is_empty() && arch != repo.arch && arch != "noarch" {
            continue;
        }
        let epoch = attr(block, "version", "epoch").unwrap_or("0");
        let ver = attr(block, "version", "ver").unwrap_or("");
        let rel = attr(block, "version", "rel").unwrap_or("");
        let version = if rel.is_empty() {
            ver.to_string()
        } else {
            format!("{ver}-{rel}")
        };
        let version = if epoch != "0" && !epoch.is_empty() {
            format!("{epoch}:{version}")
        } else {
            version
        };
        let checksum = sha256_checksum(block);
        let size: u64 = attr(block, "size", "package")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let format_section = between(block, "<format>", "</format>").unwrap_or("");
        let provides_block = between(format_section, "<rpm:provides>", "</rpm:provides>").unwrap_or("");
        let requires_block = between(format_section, "<rpm:requires>", "</rpm:requires>").unwrap_or("");
        let provides = rpm_entries(provides_block);
        let requires = rpm_entries(requires_block);
        let summary = tag_text(block, "summary").unwrap_or("");

        packages.push(Package {
            name: xml_unescape(name),
            version,
            arch: arch.to_string(),
            filename: href.to_string(),
            size,
            sha256: checksum,
            provides: normalize_list(&provides.iter().map(String::as_str).collect::<Vec<_>>()),
            depends: normalize_list(&requires.iter().map(String::as_str).collect::<Vec<_>>()),
            groups: {
                let group = tag_text(format_section, "rpm:group").unwrap_or("");
                if group.is_empty() || group.eq_ignore_ascii_case("unspecified") {
                    Vec::new()
                } else {
                    vec![group.to_string()]
                }
            },
            description: clean(&xml_unescape(summary)),
            codename: repo.codename.clone(),
        });
    }
    packages
}

/// Find the primary metadata file inside a Fedora `repomd.xml`.
pub fn rpm_primary_location(repomd: &str) -> Option<String> {
    let mut rest = repomd;
    while let Some(start) = rest.find("<data type=\"primary\"") {
        let block = &rest[start..];
        let end = block.find("</data>").map(|e| e + 7).unwrap_or(block.len());
        let block = &block[..end];
        if let Some(href) = attr(block, "location", "href") {
            return Some(href.to_string());
        }
        rest = &rest[start + end..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(fmt: Format) -> Repo {
        match fmt {
            Format::Pacman => Repo::pacman("colony", "https://e/x", "core", "x86_64", "t"),
            Format::Debian => Repo::debian("tundra", "http://d", "bookworm", "main", "amd64", "t"),
            Format::Rpm => Repo::rpm("aurora", "https://f", "x86_64", "t"),
        }
    }

    #[test]
    fn normalizes_upstream_dependencies() {
        assert_eq!(normalize_dep("glibc>=2.38").unwrap(), "glibc");
        assert_eq!(normalize_dep("libc6 (>= 2.34)").unwrap(), "libc6");
        assert_eq!(normalize_dep("libc6-dev | libc-dev").unwrap(), "libc6-dev");
        assert_eq!(normalize_dep("libm.so.6()(64bit)"), None);
        assert_eq!(normalize_dep("rpmlib(CompressedFileNames)"), None);
        assert_eq!(normalize_dep("/usr/bin/sh"), None);
        assert_eq!(normalize_dep("acl:any").unwrap(), "acl");
    }

    #[test]
    fn pacman_db_desc_parsing() {
        let desc = "%FILENAME%\nnacl-2.4.0-1-x86_64.pkg.tar.zst\n\n%NAME%\nacl\n\n%VERSION%\n2.4.0-1\n\n%DESC%\nAccess control list\n\n%CSIZE%\n150821\n\n%SHA256SUM%\nabcd\n\n%ARCH%\nx86_64\n\n%DEPENDS%\nattr\nglibc\n\n%PROVIDES%\nacl-tools=2.4.0\n\n";
        let packages = parse_pacman_db_str(desc, &repo(Format::Pacman));
        assert_eq!(packages.len(), 1);
        let p = &packages[0];
        assert_eq!(p.name, "acl");
        assert_eq!(p.version, "2.4.0-1");
        assert_eq!(p.size, 150821);
        assert_eq!(p.depends, vec!["attr", "glibc"]);
        assert_eq!(p.provides, vec!["acl-tools"]);
        assert_eq!(p.codename, "colony");
    }

    /// Helper: wrap a desc body in a real tar so the tar reader is exercised.
    fn parse_pacman_db_str(desc: &str, repo: &Repo) -> Vec<Package> {
        let mut tar = Vec::new();
        let payload = desc.as_bytes();
        let mut header = [0u8; 512];
        let name = b"acl-2.4.0-1/desc";
        header[..name.len()].copy_from_slice(name);
        header[100..108].copy_from_slice(b"0000644\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", payload.len()).as_bytes());
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let sum: u32 = header.iter().map(|&b| b as u32).sum();
        header[148..156].copy_from_slice(format!("{:06o}\0 ", sum).as_bytes());
        tar.extend_from_slice(&header);
        tar.extend_from_slice(payload);
        let pad = (512 - payload.len() % 512) % 512;
        tar.extend(std::iter::repeat(0u8).take(pad + 1024));
        parse_pacman_db(&tar, repo).expect("parse")
    }

    #[test]
    fn debian_stanza_parsing() {
        let text = "Package: acl\nVersion: 2.3.1-3\nArchitecture: amd64\nDepends: libc6 (>= 2.34), libacl1 (= 2.3.1-3)\nProvides: acl-tools\nDescription: access control list - utilities\n more text here\nFilename: pool/main/a/acl/acl_2.3.1-3_amd64.deb\nSize: 65180\nSHA256: deadbeef\n\nPackage: acl\nVersion: 2.3.1-3\nArchitecture: i386\nFilename: pool/main/a/acl/acl_i386.deb\n\n";
        let packages = parse_debian_packages(text, &repo(Format::Debian));
        assert_eq!(packages.len(), 1, "i386 stanza filtered out");
        let p = &packages[0];
        assert_eq!(p.name, "acl");
        assert_eq!(p.depends, vec!["libc6", "libacl1"]);
        assert_eq!(p.filename, "pool/main/a/acl/acl_2.3.1-3_amd64.deb");
        assert_eq!(p.description, "access control list - utilities");
    }

    #[test]
    fn rpm_primary_parsing() {
        let xml = r#"<?xml version="1.0"?>
<metadata packages="2">
<package type="rpm">
  <name>0ad</name>
  <arch>x86_64</arch>
  <version epoch="0" ver="0.27.1" rel="4.fc43"/>
  <checksum type="sha256" pkgid="YES">3f2334</checksum>
  <summary>Cross-Platform RTS</summary>
  <size package="4845339" installed="17810139" archive="17844812"/>
  <location href="Packages/0/0ad-0.27.1-4.fc43.x86_64.rpm"/>
  <format>
    <rpm:provides>
      <rpm:entry name="0ad" flags="EQ" epoch="0" ver="0.27.1" rel="4.fc43"/>
      <rpm:entry name="application(0ad.desktop)"/>
    </rpm:provides>
    <rpm:requires>
      <rpm:entry name="0ad-data" flags="EQ" epoch="0" ver="0.27.1"/>
      <rpm:entry name="libcurl.so.4()(64bit)"/>
      <rpm:entry name="/usr/bin/sh"/>
    </rpm:requires>
  </format>
</package>
<package type="rpm">
  <name>aarch64-only</name>
  <arch>aarch64</arch>
  <version epoch="0" ver="1" rel="1"/>
  <location href="Packages/a/aarch64-only.rpm"/>
</package>
</metadata>"#;
        let packages = parse_rpm_primary(xml, &repo(Format::Rpm));
        assert_eq!(packages.len(), 1);
        let p = &packages[0];
        assert_eq!(p.name, "0ad");
        assert_eq!(p.version, "0.27.1-4.fc43");
        assert_eq!(p.size, 4_845_339);
        assert_eq!(p.filename, "Packages/0/0ad-0.27.1-4.fc43.x86_64.rpm");
        assert_eq!(p.depends, vec!["0ad-data"]);
        assert_eq!(p.provides, vec!["0ad"]);
        assert_eq!(p.sha256, "3f2334");
    }

    #[test]
    fn repomd_primary_location() {
        let repomd = r#"<repomd><data type="primary"><location href="repodata/abc-primary.xml.zst"/></data>
        <data type="filelists"><location href="repodata/def-filelists.xml.gz"/></data></repomd>"#;
        assert_eq!(
            rpm_primary_location(repomd).unwrap(),
            "repodata/abc-primary.xml.zst"
        );
    }

    #[test]
    fn index_roundtrip() {
        let dir = std::env::temp_dir().join(format!("forst-idx-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("colony.idx");
        let pkg = Package {
            name: "acl".into(),
            version: "2.4.0-1".into(),
            arch: "x86_64".into(),
            filename: "acl.pkg.tar.zst".into(),
            size: 10,
            sha256: "aa".into(),
            provides: vec!["acl-tools".into()],
            depends: vec!["glibc".into()],
            groups: vec!["base".into()],
            description: "access control".into(),
            codename: "colony".into(),
        };
        let index = Index::new("colony", vec![pkg]);
        index.save(&path).unwrap();
        let back = Index::load(&path).unwrap();
        assert_eq!(back.codename, "colony");
        assert_eq!(back.get("acl").unwrap().depends, vec!["glibc"]);
        assert_eq!(back.get("acl").unwrap().groups, vec!["base"]);
        assert!(back.get("nope").is_none());
        fs::remove_dir_all(&dir).ok();
    }

    /// A small deterministic corpus for the three parsers. It combines
    /// truncations of valid structures with pseudo-random noise; the contract
    /// is "never panic, never over-allocate", not "extract meaning from junk".
    #[test]
    fn parsers_survive_hostile_input() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        fn tar_bytes(desc: &str) -> Vec<u8> {
            let mut tar = Vec::new();
            let payload = desc.as_bytes();
            let mut header = [0u8; 512];
            let name = b"acl-2.4.0-1/desc";
            header[..name.len()].copy_from_slice(name);
            header[100..108].copy_from_slice(b"0000644\0");
            header[124..136].copy_from_slice(format!("{:011o}\0", payload.len()).as_bytes());
            header[156] = b'0';
            header[257..263].copy_from_slice(b"ustar\0");
            header[263..265].copy_from_slice(b"00");
            let sum: u32 = header.iter().map(|&b| b as u32).sum();
            header[148..156].copy_from_slice(format!("{:06o}\0 ", sum).as_bytes());
            tar.extend_from_slice(&header);
            tar.extend_from_slice(payload);
            let pad = (512 - payload.len() % 512) % 512;
            tar.extend(std::iter::repeat(0u8).take(pad + 1024));
            tar
        }

        let desc = "%FILENAME%\nacl.pkg.tar.zst\n\n%NAME%\nacl\n\n%VERSION%\n1\n\n%ARCH%\nx86_64\n\n";
        let valid_tar = tar_bytes(desc);
        let valid_deb = "Package: acl\nVersion: 1\nArchitecture: amd64\nFilename: pool/a.deb\nSHA256: aa\n\n";
        let valid_rpm = "<package><name>acl</name><arch>x86_64</arch><version epoch=\"0\" ver=\"1\" rel=\"1\"/><location href=\"a.rpm\"/></package>";

        let mut seed: u64 = 0x5eed_c0de_f00d_beef;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            seed
        };

        for i in 0..96usize {
            let len = (next() as usize % 1024) + i % 7;
            let noise: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let cut = next() as usize % (valid_tar.len() + 1);
            let tar_cut = valid_tar[..cut].to_vec();
            assert!(catch_unwind(AssertUnwindSafe(|| {
                let _ = parse_pacman_db(&noise, &repo(Format::Pacman));
                let _ = parse_pacman_db(&tar_cut, &repo(Format::Pacman));
                let text = String::from_utf8_lossy(&noise);
                let _ = parse_debian_packages(&text, &repo(Format::Debian));
                let deb_cut = &valid_deb[..valid_deb.len().min(cut)];
                let _ = parse_debian_packages(deb_cut, &repo(Format::Debian));
                let _ = parse_rpm_primary(&text, &repo(Format::Rpm));
                let rpm_cut = &valid_rpm[..valid_rpm.len().min(cut)];
                let _ = parse_rpm_primary(rpm_cut, &repo(Format::Rpm));
            }))
            .is_ok(), "parser panicked on iteration {i}");
        }
    }
}
