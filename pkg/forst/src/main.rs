//! forst -- the Forsteri package manager.
//!
//! One small binary, five codenames, one ledger.

mod config;
mod index;
mod install;
mod resolve;
mod sign;
mod sync;
mod util;

use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::exit;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
forst -- Forsteri package manager

usage: forst [options] <command> [arguments]

commands:
  repos                      list codenames and their mirrors
  sync [codename...]         refresh repository indexes
  search <word>              search every synced codename
  info <spec>                show one package
  install <spec...>          resolve, download, verify, extract
  why <spec...>              show the plan without installing
  remove <name...>           remove packages and their recorded files
  list                       installed packages
  files <name>               files owned by an installed package
  verify [name...]           re-hash installed files and report drift

a spec is  name  or  codename:name  (also codename/name), e.g.
  forst install htop                from the base order (colony, ice, nest)
  forst install tundra:htop         the Debian build, as an asset
  forst install aurora:ripgrep      the Fedora build, as an asset

options:
  --root <dir>          install into a mounted root (default /)
  --db <dir>            ledger directory     (default <root>/var/lib/forst)
  --cache <dir>         cache directory      (default /var/cache/forst)
  --conf <file>         repos.conf           (default /etc/forst/repos.conf)
  -r, --repo <codename> restrict search to one codename
      --dry-run         plan only
      --strict          fail on unresolved dependencies
      --allow-foreign-system
                        permit a foreign codename to replace base packages
  -y, --yes             do not ask for confirmation
  -v, --verbose         more output
  -h, --help            this text
  -V, --version         version

environment: FORST_ROOT, FORST_DB, FORST_CACHE, FORST_CONF
";

#[derive(Default)]
struct Opts {
    root: Option<PathBuf>,
    cache: Option<PathBuf>,
    db: Option<PathBuf>,
    conf: Option<PathBuf>,
    repo: Option<String>,
    verbose: bool,
    dry_run: bool,
    strict: bool,
    allow_foreign_system: bool,
    yes: bool,
    positionals: Vec<String>,
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut opts = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut take_value = |name: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--root" => opts.root = Some(PathBuf::from(take_value("--root")?)),
            "--cache" => opts.cache = Some(PathBuf::from(take_value("--cache")?)),
            "--db" => opts.db = Some(PathBuf::from(take_value("--db")?)),
            "--conf" => opts.conf = Some(PathBuf::from(take_value("--conf")?)),
            "-r" | "--repo" => opts.repo = Some(take_value("--repo")?),
            "--dry-run" | "-n" => opts.dry_run = true,
            "--strict" => opts.strict = true,
            "--allow-foreign-system" => opts.allow_foreign_system = true,
            "-y" | "--yes" => opts.yes = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                exit(0);
            }
            "-V" | "--version" => {
                println!("forst {VERSION}");
                exit(0);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option {other}"));
            }
            _ => opts.positionals.push(arg),
        }
        i += 1;
    }
    Ok(opts)
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt} [Y/n] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes")
}

fn main() {
    util::default_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("forst: error: {e}");
            exit(1);
        }
    }
}

fn run(args: &[String]) -> util::Result<()> {
    let opts = parse_args(args).map_err(util::Error::new)?;
    let Some(command) = opts.positionals.first().cloned() else {
        print!("{USAGE}");
        return Ok(());
    };
    let rest: Vec<String> = opts.positionals[1..].to_vec();

    let root = opts
        .root
        .clone()
        .or_else(|| env_path("FORST_ROOT"))
        .unwrap_or_else(|| PathBuf::from("/"));
    let cache = opts
        .cache
        .clone()
        .or_else(|| env_path("FORST_CACHE"))
        .unwrap_or_else(|| PathBuf::from("/var/cache/forst"));
    let db = opts
        .db
        .clone()
        .or_else(|| env_path("FORST_DB"))
        .unwrap_or_else(|| {
            if root == Path::new("/") {
                PathBuf::from("/var/lib/forst")
            } else {
                root.join("var/lib/forst")
            }
        });
    let conf = opts
        .conf
        .clone()
        .or_else(|| env_path("FORST_CONF"))
        .unwrap_or_else(|| PathBuf::from("/etc/forst/repos.conf"));

    let repos = config::load(&conf)?;

    match command.as_str() {
        "repos" => cmd_repos(&repos, &cache),
        "sync" => cmd_sync(&repos, &cache, &rest, opts.verbose),
        "search" => cmd_search(&repos, &cache, &rest, opts.repo.as_deref()),
        "info" => cmd_info(&repos, &cache, &db, &rest),
        "install" | "add" => cmd_install(&opts, &repos, &root, &cache, &db, &rest),
        "why" => {
            let mut opts = opts;
            opts.dry_run = true;
            cmd_install(&opts, &repos, &root, &cache, &db, &rest)
        }
        "remove" | "rm" => cmd_remove(&root, &db, &rest, opts.yes),
        "list" | "installed" => cmd_list(&db),
        "files" => cmd_files(&db, &rest),
        "verify" | "check" => cmd_verify(&root, &db, &rest),
        other => Err(util::Error::new(format!(
            "unknown command {other:?}; try `forst --help`"
        ))),
    }
}

fn cmd_repos(repos: &[config::Repo], cache: &Path) -> util::Result<()> {
    println!(
        "{:<10} {:<8} {:<22} {:>9}  {}",
        "CODENAME", "FORMAT", "SOURCE", "PACKAGES", "DESCRIPTION"
    );
    for repo in repos {
        let count = index::Index::load(&repo.index_path(cache))
            .map(|i| i.packages.len().to_string())
            .unwrap_or_else(|_| "-".to_string());
        let dot = if repo.enabled { ' ' } else { '!' };
        println!(
            "{dot}{:<9} {:<8} {:<22} {:>9}  {}",
            repo.codename,
            repo.format.as_str(),
            repo.summary(),
            count,
            repo.description
        );
    }
    println!();
    println!("base codenames (pacman): safe to install, share Forsteri's ledger");
    println!("asset codenames (deb/rpm): files only, guarded against replacing the base");
    Ok(())
}

fn cmd_sync(repos: &[config::Repo], cache: &Path, only: &[String], verbose: bool) -> util::Result<()> {
    if !only.is_empty() {
        for codename in only {
            if config::find(repos, codename).is_none() {
                return Err(util::Error::new(format!(
                    "unknown codename {codename:?}; see `forst repos`"
                )));
            }
        }
    }
    let outcomes = sync::sync_all(repos, cache, only, verbose);
    if outcomes.is_empty() {
        return Err(util::Error::new("no enabled codenames matched"));
    }
    let mut failed = 0;
    for outcome in &outcomes {
        match &outcome.error {
            None => println!("{:>9} packages  {}", outcome.count, outcome.codename),
            Some(e) => {
                failed += 1;
                println!("{:>9}           {} ({e})", "failed", outcome.codename);
            }
        }
    }
    println!(
        "synced {} codename(s), {} failed; indexes in {}",
        outcomes.len(),
        failed,
        cache.join("index").display()
    );
    if failed == outcomes.len() {
        return Err(util::Error::new("every codename failed to sync"));
    }
    Ok(())
}

fn cmd_search(
    repos: &[config::Repo],
    cache: &Path,
    words: &[String],
    repo_filter: Option<&str>,
) -> util::Result<()> {
    if words.is_empty() {
        return Err(util::Error::new("search needs a word, e.g. `forst search htop`"));
    }
    let query = words.join(" ").to_ascii_lowercase();
    let catalog = resolve::Catalog::open(repos.to_vec(), cache, false)?;
    let mut shown = 0;
    let mut truncated = false;
    for repo in config::default_order(&catalog.repos) {
        if let Some(filter) = repo_filter {
            if !repo.codename.eq_ignore_ascii_case(filter) {
                continue;
            }
        }
        let Some(index) = catalog.indexes.get(&repo.codename) else {
            continue;
        };
        for pkg in &index.packages {
            let hit = pkg.name.to_ascii_lowercase().contains(&query)
                || pkg.description.to_ascii_lowercase().contains(&query);
            if !hit {
                continue;
            }
            if shown >= 200 {
                truncated = true;
                break;
            }
            let desc: String = pkg.description.chars().take(70).collect();
            println!(
                "{:<12} {:<28} {:<14} {}",
                pkg.codename, pkg.name, pkg.version, desc
            );
            shown += 1;
        }
    }
    if shown == 0 {
        println!("no matches for {query:?}");
    } else if truncated {
        println!("... (showing the first 200; narrow the query or use -r <codename>)");
    }
    Ok(())
}

fn cmd_info(
    repos: &[config::Repo],
    cache: &Path,
    db: &Path,
    words: &[String],
) -> util::Result<()> {
    let Some(raw) = words.first() else {
        return Err(util::Error::new("info needs a spec, e.g. `forst info tundra:htop`"));
    };
    let catalog = resolve::Catalog::open(repos.to_vec(), cache, false)?;
    let spec = resolve::parse_spec(raw, &repos);
    if spec.group {
        return Err(util::Error::new(format!(
            "@{} is a package group; use `forst install @{}` or `forst search {}`",
            spec.name, spec.name, spec.name
        )));
    }
    let found = match &spec.codename {
        Some(codename) => catalog.find_in(&spec.name, codename).ok_or_else(|| {
            let elsewhere = catalog.codenames_with(&spec.name);
            let hint = if elsewhere.is_empty() {
                String::new()
            } else {
                format!(" (it exists in: {})", elsewhere.join(", "))
            };
            util::Error::new(format!(
                "no package {:?} in codename {codename}{hint}",
                spec.name
            ))
        })?,
        None => catalog.find(&spec.name, None).ok_or_else(|| {
            util::Error::new(format!(
                "no package {:?} in any synced codename (try `forst search {}`)",
                spec.name, spec.name
            ))
        })?,
    };
    let pkg = &found;
    println!("Name         : {}", pkg.name);
    println!("Codename     : {} ({})", pkg.codename, pkg.arch);
    println!("Version      : {}", pkg.version);
    println!("Size         : {}", util::human_size(pkg.size));
    println!("Description  : {}", pkg.description);
    println!("File         : {}", pkg.filename);
    println!("SHA256       : {}", pkg.sha256);
    if !pkg.depends.is_empty() {
        println!("Depends      : {}", pkg.depends.join(" "));
    }
    if !pkg.provides.is_empty() {
        println!("Provides     : {}", pkg.provides.join(" "));
    }
    let installed = install::get_installed(db, &pkg.name);
    if let Some(meta) = installed {
        println!(
            "Installed    : {} from {}",
            meta.version, meta.codename
        );
    }
    Ok(())
}

fn cmd_install(
    opts: &Opts,
    repos: &[config::Repo],
    root: &Path,
    cache: &Path,
    db: &Path,
    specs: &[String],
) -> util::Result<()> {
    if specs.is_empty() {
        return Err(util::Error::new(
            "install needs at least one spec, e.g. `forst install colony:htop`",
        ));
    }
    let catalog = resolve::Catalog::open(repos.to_vec(), cache, true)?;
    let installed_metas = install::installed(db)?;
    let installed = resolve::installed_names(&installed_metas);
    let specs: Vec<resolve::Spec> = specs
        .iter()
        .map(|raw| resolve::parse_spec(raw, repos))
        .collect();

    let plan = resolve::resolve(&catalog, &specs, &installed, opts.strict)?;

    // The guard applies to what the user explicitly asked for. Dependencies of
    // asset packages that look like base-system packages are skipped during
    // resolution instead (the base provides them).
    for spec in &specs {
        if spec.group {
            continue;
        }
        let found = match &spec.codename {
            Some(codename) => catalog.find_in(&spec.name, codename),
            None => catalog.find(&spec.name, None),
        };
        if let Some(pkg) = found {
            if let Some(reason) = resolve::foreign_guard(&pkg, &catalog, opts.allow_foreign_system) {
                return Err(util::Error::new(reason));
            }
        }
    }

    for missing in &plan.missing {
        util::warn(&format!(
            "unresolved dependency {missing:?} (not in any synced codename)"
        ));
    }

    let mut todo = Vec::new();
    for pkg in &plan.packages {
        if let Some(meta) = installed_metas.iter().find(|m| m.name == pkg.name) {
            if meta.version == pkg.version {
                if opts.verbose {
                    println!("{}: already installed at {}", pkg.spec(), pkg.version);
                }
                continue;
            }
            todo.push((pkg.clone(), true));
        } else {
            todo.push((pkg.clone(), false));
        }
    }

    if todo.is_empty() {
        println!("nothing to do: already installed");
        return Ok(());
    }

    let total: u64 = todo.iter().map(|(p, _)| p.size).sum();
    println!(
        "plan: {} package(s), {} to download",
        todo.len(),
        util::human_size(total)
    );
    for (pkg, upgrade) in &todo {
        println!(
            "  {:<12} {:<28} {:<16} {}",
            pkg.codename,
            pkg.name,
            pkg.version,
            if *upgrade { "(upgrade)" } else { "" }
        );
    }

    if opts.dry_run {
        return Ok(());
    }
    if !opts.yes && !confirm("install?") {
        println!("aborted");
        return Ok(());
    }

    for (pkg, _) in &todo {
        let repo = config::find(repos, &pkg.codename).ok_or_else(|| {
            util::Error::new(format!("codename {} disappeared", pkg.codename))
        })?;
        let report = install::install_package(repo, pkg, root, cache, db, opts.verbose)?;
        println!(
            "installed {} {} ({} files{})",
            pkg.spec(),
            pkg.version,
            report.files,
            if report.downloaded { ", fetched" } else { ", cached" }
        );
    }
    Ok(())
}

fn cmd_remove(root: &Path, db: &Path, names: &[String], yes: bool) -> util::Result<()> {
    if names.is_empty() {
        return Err(util::Error::new("remove needs a package name"));
    }
    for name in names {
        if install::get_installed(db, name).is_none() {
            return Err(util::Error::new(format!("{name} is not installed")));
        }
    }
    if !yes && !confirm(&format!("remove {}?", names.join(", "))) {
        println!("aborted");
        return Ok(());
    }
    for name in names {
        let removed = install::remove_package(db, root, name)?;
        println!("removed {name} ({removed} files)");
    }
    Ok(())
}

fn cmd_list(db: &Path) -> util::Result<()> {
    let metas = install::installed(db)?;
    if metas.is_empty() {
        println!("no packages installed in {}", db.display());
        return Ok(());
    }
    println!("{:<30} {:<18} {}", "NAME", "VERSION", "CODENAME");
    for meta in &metas {
        println!("{:<30} {:<18} {}", meta.name, meta.version, meta.codename);
    }
    println!("\n{} package(s)", metas.len());
    Ok(())
}

fn cmd_verify(root: &Path, db: &Path, names: &[String]) -> util::Result<()> {
    let report = install::verify(db, root, names)?;
    for drift in &report.drift {
        println!("{:<10} {} ({})", drift.reason, drift.path, drift.package);
    }
    println!(
        "verified {} file(s) in {} package(s); {} unhashed legacy entr{}",
        report.checked,
        report.packages,
        report.unhashed,
        if report.unhashed == 1 { "y" } else { "ies" }
    );
    if report.unhashed > 0 {
        util::warn("legacy ledger entries have no digest; reinstall those packages to enable verification");
    }
    if !report.drift.is_empty() {
        return Err(util::Error::new(format!(
            "{} installed file(s) drifted",
            report.drift.len()
        )));
    }
    println!("no drift");
    Ok(())
}

fn cmd_files(db: &Path, names: &[String]) -> util::Result<()> {
    let Some(name) = names.first() else {
        return Err(util::Error::new("files needs a package name"));
    };
    let mut set: HashSet<String> = HashSet::new();
    for entry in install::list_files(db, name)? {
        // The ledger stores relative paths; people expect absolute ones.
        let path = if entry.path.starts_with('/') {
            entry.path.clone()
        } else {
            format!("/{}", entry.path)
        };
        set.insert(format!("{} {}", entry.kind.tag(), path));
    }
    let mut lines: Vec<&String> = set.iter().collect();
    lines.sort();
    for line in lines {
        println!("{line}");
    }
    Ok(())
}
