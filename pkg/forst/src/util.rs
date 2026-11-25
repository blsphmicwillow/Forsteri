//! Small, dependency-free primitives used by forst.
//!
//! Everything here is deliberately hand written: forst must be a single small
//! binary that works on a bare Forsteri/Arch system with no library beyond
//! libc. Compression is delegated to the standard system tools (`zstd`, `xz`,
//! `gzip`, `bzip2`), which are always present on the base system.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Error(pub String);

impl Error {
    pub fn new<S: Into<String>>(s: S) -> Error {
        Error(s.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn err<T, S: Into<String>>(s: S) -> Result<T> {
    Err(Error::new(s))
}

/// Print a warning to stderr.
pub fn warn(msg: &str) {
    eprintln!("forst: warning: {msg}");
}

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4)
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buflen: usize,
    total: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0u8; 64],
            buflen: 0,
            total: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buflen > 0 {
            let take = std::cmp::min(64 - self.buflen, data.len());
            self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
            self.buflen += take;
            data = &data[take..];
            if self.buflen == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buflen = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buflen = data.len();
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bitlen = self.total.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buflen != 56 {
            self.update(&[0x00]);
        }
        // update() counted the padding bytes; that is harmless because bitlen
        // was captured before padding started.
        let mut lenbuf = [0u8; 8];
        lenbuf.copy_from_slice(&bitlen.to_be_bytes());
        self.buf[56..64].copy_from_slice(&lenbuf);
        let block = self.buf;
        self.compress(&block);
        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

#[cfg(test)]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finish())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = BufReader::new(File::open(path)?);
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finish()))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

// ---------------------------------------------------------------------------
// temp files
// ---------------------------------------------------------------------------

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct TempFile {
    pub path: PathBuf,
    remove: bool,
}

impl TempFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.remove {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn temp_path(dir: &Path, stem: &str) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    dir.join(format!(".forst-{}-{}-{}.{}", std::process::id(), nanos, n, stem))
}

pub fn write_temp(dir: &Path, data: &[u8], stem: &str) -> Result<TempFile> {
    fs::create_dir_all(dir)?;
    let path = temp_path(dir, stem);
    let mut f = File::create(&path)?;
    f.write_all(data)?;
    Ok(TempFile { path, remove: true })
}

// ---------------------------------------------------------------------------
// decompression
// ---------------------------------------------------------------------------

/// Detect compression from magic bytes. Returns the tool and its arguments.
fn detect(head: &[u8]) -> Option<(&'static str, &'static [&'static str])> {
    if head.len() >= 4 && head[..4] == [0x28, 0xb5, 0x2f, 0xfd] {
        return Some(("zstd", &["-dc", "-q"]));
    }
    if head.len() >= 6 && head[..6] == [0xfd, b'7', b'z', b'X', b'Z', 0x00] {
        return Some(("xz", &["-dc"]));
    }
    if head.len() >= 2 && head[..2] == [0x1f, 0x8b] {
        return Some(("gzip", &["-dc"]));
    }
    if head.len() >= 3 && &head[..3] == b"BZh" {
        return Some(("bzip2", &["-dc"]));
    }
    if head.len() >= 3 && head[..3] == [0x5d, 0x00, 0x00] {
        return Some(("xz", &["--format=lzma", "-dc"]));
    }
    None
}

pub fn compression_name(head: &[u8]) -> &'static str {
    match detect(head) {
        Some((tool, _)) => tool,
        None => "none",
    }
}

/// Decompress `path` into a temporary file (in `tmpdir`) and return it. If the
/// file is not compressed the original path is returned unmanaged.
pub fn decompress_path(path: &Path, tmpdir: &Path) -> Result<TempFile> {
    let mut head = [0u8; 8];
    let mut f = File::open(path)?;
    let n = f.read(&mut head)?;
    let Some((tool, args)) = detect(&head[..n]) else {
        return Ok(TempFile {
            path: path.to_path_buf(),
            remove: false,
        });
    };
    f.seek(SeekFrom::Start(0))?;
    drop(f);

    fs::create_dir_all(tmpdir)?;
    let out_path = temp_path(tmpdir, "raw");
    let out = File::create(&out_path)?;
    let child = Command::new(tool)
        .args(args)
        .arg(path)
        .stdout(Stdio::from(out))
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&out_path);
            return err(format!("cannot run {tool} to decompress {}: {e}", path.display()));
        }
    };
    let mut stderr = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut stderr);
    }
    let status = child.wait()?;
    if !status.success() {
        let _ = fs::remove_file(&out_path);
        return err(format!(
            "{tool} failed on {}: {}",
            path.display(),
            stderr.trim()
        ));
    }
    Ok(TempFile {
        path: out_path,
        remove: true,
    })
}

/// Decompress a byte slice in memory.
pub fn decompress_bytes(data: &[u8]) -> Result<Vec<u8>> {
    let Some((tool, args)) = detect(data) else {
        return Ok(data.to_vec());
    };
    let mut child = Command::new(tool)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::new(format!("cannot run {tool}: {e}")))?;
    let mut stdin = child.stdin.take().ok_or_else(|| Error::new("no stdin"))?;
    let input = data.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut out = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let mut r = stdout;
        r.read_to_end(&mut out)?;
    }
    let _ = writer.join();
    let status = child.wait()?;
    if !status.success() {
        return err(format!("{tool} failed"));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// path safety
// ---------------------------------------------------------------------------

/// Turn an archive member name into a relative, safe path.
pub fn safe_rel(name: &str) -> Option<PathBuf> {
    let mut trimmed = name;
    while let Some(rest) = trimmed.strip_prefix("./") {
        trimmed = rest;
    }
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    let mut out = PathBuf::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn ensure_dirs(root: &Path, rel: &Path) -> Result<()> {
    let mut cur = root.to_path_buf();
    for comp in rel.components() {
        cur.push(comp);
        match fs::symlink_metadata(&cur) {
            Ok(md) => {
                if md.file_type().is_symlink() {
                    return err(format!(
                        "refusing to write through symlink {}",
                        cur.display()
                    ));
                }
                if !md.is_dir() {
                    return err(format!("{} exists and is not a directory", cur.display()));
                }
            }
            Err(_) => fs::create_dir(&cur)?,
        }
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = mode & 0o7777;
    if let Err(_e) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
        // Unprivileged installs cannot set setuid/setgid bits.
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsKind {
    File,
    Dir,
    Symlink,
    Hardlink,
}

impl FsKind {
    pub fn tag(self) -> char {
        match self {
            FsKind::File => 'f',
            FsKind::Dir => 'd',
            FsKind::Symlink => 'l',
            FsKind::Hardlink => 'h',
        }
    }

    pub fn from_tag(c: char) -> FsKind {
        match c {
            'd' => FsKind::Dir,
            'l' => FsKind::Symlink,
            'h' => FsKind::Hardlink,
            _ => FsKind::File,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FsEntry {
    pub kind: FsKind,
    pub path: String,
    /// SHA256 of the installed regular file. Empty for directories, symlinks
    /// and ledgers written by older forst versions.
    pub sha256: String,
}

fn place(
    root: &Path,
    rel: &Path,
    kind: char,
    mode: u32,
    data: &[u8],
    link: &str,
) -> Result<FsKind> {
    let full = root.join(rel);
    match kind {
        '5' => {
            ensure_dirs(root, rel)?;
            let _ = set_mode(&full, mode);
            Ok(FsKind::Dir)
        }
        '2' => {
            if let Some(parent) = rel.parent() {
                ensure_dirs(root, parent)?;
            }
            let target = if link.is_empty() {
                cstr_bytes(data)
            } else {
                link.to_string()
            };
            match fs::symlink_metadata(&full) {
                Ok(md) if md.is_dir() && !md.file_type().is_symlink() => {
                    fs::remove_dir_all(&full)?;
                }
                Ok(_) => {
                    fs::remove_file(&full)?;
                }
                Err(_) => {}
            }
            std::os::unix::fs::symlink(&target, &full)?;
            Ok(FsKind::Symlink)
        }
        '1' => {
            if let Some(parent) = rel.parent() {
                ensure_dirs(root, parent)?;
            }
            let Some(src_rel) = safe_rel(link) else {
                return err(format!("hardlink {} -> {}", rel.display(), link));
            };
            let _ = fs::remove_file(&full);
            fs::hard_link(root.join(src_rel), &full)?;
            Ok(FsKind::Hardlink)
        }
        _ => {
            if let Some(parent) = rel.parent() {
                ensure_dirs(root, parent)?;
            }
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&full)?;
            f.write_all(data)?;
            drop(f);
            set_mode(&full, mode)?;
            Ok(FsKind::File)
        }
    }
}

// ---------------------------------------------------------------------------
// tar
// ---------------------------------------------------------------------------

/// Upper bound for one buffered archive member. Package payloads larger than
/// this must be split by their packager; refusing them keeps a hostile header
/// from turning into a multi-gigabyte allocation.
pub const MAX_MEMBER: usize = 64 * 1024 * 1024;

pub struct TarEntry {
    pub name: String,
    pub kind: char,
    pub mode: u32,
    pub link: String,
    pub data: Vec<u8>,
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn cstr_bytes(bytes: &[u8]) -> String {
    cstr(bytes)
}

fn tar_number(field: &[u8]) -> u64 {
    if !field.is_empty() && field[0] & 0x80 != 0 {
        let mut v: u64 = 0;
        for (i, b) in field.iter().enumerate() {
            if i == 0 {
                v = (b & 0x7f) as u64;
            } else {
                v = (v << 8) | *b as u64;
            }
        }
        return v;
    }
    let s = String::from_utf8_lossy(field);
    let s = s.trim_matches(|c: char| c == '\0' || c == ' ' || c == '\n');
    if s.is_empty() {
        return 0;
    }
    u64::from_str_radix(s, 8).unwrap_or(0)
}

fn read_full_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(filled == buf.len())
}

fn skip_bytes<R: Read>(r: &mut R, n: usize) -> Result<()> {
    if n == 0 {
        return Ok(());
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(())
}

fn parse_pax(data: &[u8], path: &mut Option<String>, link: &mut Option<String>) {
    let text = String::from_utf8_lossy(data);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let sp = match bytes[i..].iter().position(|&b| b == b' ') {
            Some(p) => i + p,
            None => break,
        };
        let len: usize = match text[i..sp].trim().parse() {
            Ok(v) => v,
            Err(_) => break,
        };
        if len == 0 || len > bytes.len() - i {
            break;
        }
        let record = &text[sp + 1..i + len];
        let record = record.trim_end_matches('\n');
        if let Some(v) = record.strip_prefix("path=") {
            *path = Some(v.to_string());
        } else if let Some(v) = record.strip_prefix("linkpath=") {
            *link = Some(v.to_string());
        }
        i += len;
    }
}

pub fn tar_foreach<R: Read>(
    reader: &mut R,
    f: &mut dyn FnMut(&TarEntry) -> Result<()>,
) -> Result<()> {
    let mut long_name: Option<String> = None;
    let mut long_link: Option<String> = None;
    let mut pax_path: Option<String> = None;
    let mut pax_link: Option<String> = None;

    loop {
        let mut hdr = [0u8; 512];
        if !read_full_or_eof(reader, &mut hdr)? {
            break;
        }
        if hdr.iter().all(|&b| b == 0) {
            break;
        }
        let size = tar_number(&hdr[124..136]) as usize;
        if size > MAX_MEMBER {
            return err(format!("tar: member size {size} exceeds the {MAX_MEMBER}-byte limit"));
        }
        let mode = tar_number(&hdr[100..108]) as u32;
        let mut kind = hdr[156] as char;
        if kind == '\0' {
            kind = '0';
        }
        let mut name = cstr(&hdr[0..100]);
        let prefix = cstr(&hdr[345..500]);
        if !prefix.is_empty() {
            name = format!("{prefix}/{name}");
        }
        let mut link = cstr(&hdr[157..257]);

        let mut data = vec![0u8; size];
        if size > 0 && !read_full_or_eof(reader, &mut data)? {
            return err("tar: truncated member");
        }
        let pad = (512 - (size % 512)) % 512;
        skip_bytes(reader, pad)?;

        match kind {
            'L' => {
                long_name = Some(cstr_bytes(&data));
                continue;
            }
            'K' => {
                long_link = Some(cstr_bytes(&data));
                continue;
            }
            'x' | 'X' => {
                parse_pax(&data, &mut pax_path, &mut pax_link);
                continue;
            }
            'g' => continue,
            _ => {}
        }

        if let Some(n) = long_name.take() {
            name = n;
        }
        if let Some(n) = long_link.take() {
            link = n;
        }
        if let Some(n) = pax_path.take() {
            name = n;
        }
        if let Some(n) = pax_link.take() {
            link = n;
        }

        f(&TarEntry {
            name,
            kind,
            mode,
            link,
            data,
        })?;
    }
    Ok(())
}

/// Extract a tar stream into `root`. When `skip_meta` is set, members whose
/// name starts with a dot (pacman's `.PKGINFO`, `.MTREE`, `.INSTALL`, ...) are
/// ignored.
pub fn tar_extract<R: Read>(reader: &mut R, root: &Path, skip_meta: bool) -> Result<Vec<FsEntry>> {
    let mut out = Vec::new();
    tar_foreach(reader, &mut |e| {
        if skip_meta && e.name.starts_with('.') && !e.name.starts_with("./") {
            return Ok(());
        }
        // A bare "./" member is the archive's own root; nothing to create.
        if e.name.trim_start_matches("./").is_empty() {
            return Ok(());
        }
        let Some(rel) = safe_rel(&e.name) else {
            warn(&format!("skipping unsafe archive path {:?}", e.name));
            return Ok(());
        };
        let path = rel.to_string_lossy().to_string();
        match e.kind {
            '5' => {
                ensure_dirs(root, &rel)?;
                out.push(FsEntry {
                    kind: FsKind::Dir,
                    path,
                    sha256: String::new(),
                });
            }
            '2' | '1' | '0' | '7' | '\0' => {
                let kind = place(root, &rel, e.kind, e.mode, &e.data, &e.link)?;
                out.push(FsEntry {
                    kind,
                    path,
                    sha256: String::new(),
                });
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// ar / deb
// ---------------------------------------------------------------------------

pub fn ar_foreach<F>(path: &Path, mut f: F) -> Result<()>
where
    F: FnMut(&str, &[u8]) -> Result<()>,
{
    let mut file = File::open(path)?;
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != b"!<arch>\n" {
        return err(format!("{}: not an ar archive", path.display()));
    }
    loop {
        let mut hdr = [0u8; 60];
        match file.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let name = String::from_utf8_lossy(&hdr[0..16])
            .trim()
            .trim_end_matches('/')
            .to_string();
        let size: usize = String::from_utf8_lossy(&hdr[48..58])
            .trim()
            .parse()
            .unwrap_or(0);
        if size > MAX_MEMBER {
            return err(format!("ar: member size {size} exceeds the {MAX_MEMBER}-byte limit"));
        }
        let mut data = vec![0u8; size];
        if size > 0 {
            file.read_exact(&mut data)?;
        }
        if size % 2 == 1 {
            let mut pad = [0u8; 1];
            let _ = file.read_exact(&mut pad);
        }
        f(&name, &data)?;
    }
    Ok(())
}

pub fn deb_extract(path: &Path, root: &Path, tmpdir: &Path) -> Result<Vec<FsEntry>> {
    let mut data: Option<Vec<u8>> = None;
    ar_foreach(path, |name, bytes| {
        if name.starts_with("data.tar") {
            data = Some(bytes.to_vec());
        }
        Ok(())
    })?;
    let data = data.ok_or_else(|| Error::new(format!("{}: no data.tar member", path.display())))?;
    let tmp = write_temp(tmpdir, &data, "deb")?;
    let raw = decompress_path(tmp.path(), tmpdir)?;
    let mut reader = BufReader::new(File::open(raw.path())?);
    tar_extract(&mut reader, root, false)
}

// ---------------------------------------------------------------------------
// rpm
// ---------------------------------------------------------------------------

fn skip_rpm_header(f: &mut File, align: u64) -> Result<()> {
    let start = f.stream_position()?;
    let mut h = [0u8; 16];
    f.read_exact(&mut h)?;
    if &h[0..4] != b"\x8e\xad\xe8\x01" {
        return err("rpm: bad header magic");
    }
    let nidx = u32::from_be_bytes([h[8], h[9], h[10], h[11]]) as u64;
    let hsize = u32::from_be_bytes([h[12], h[13], h[14], h[15]]) as u64;
    let mut end = start + 16 + nidx * 16 + hsize;
    if align > 1 {
        let len = end - start;
        let pad = (align - (len % align)) % align;
        end += pad;
    }
    f.seek(SeekFrom::Start(end))?;
    Ok(())
}

/// Offset of the compressed cpio payload inside an RPM.
pub fn rpm_payload_offset(path: &Path) -> Result<u64> {
    let mut f = File::open(path)?;
    let mut lead = [0u8; 96];
    f.read_exact(&mut lead)?;
    if &lead[0..4] != b"\xed\xab\xee\xdb" {
        return err(format!("{}: not an rpm (bad lead)", path.display()));
    }
    skip_rpm_header(&mut f, 8)?; // signature header
    skip_rpm_header(&mut f, 1)?; // main header
    Ok(f.stream_position()?)
}

pub fn rpm_extract(path: &Path, root: &Path, tmpdir: &Path) -> Result<Vec<FsEntry>> {
    let offset = rpm_payload_offset(path)?;
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut payload = Vec::new();
    f.read_to_end(&mut payload)?;
    let tmp = write_temp(tmpdir, &payload, "rpm")?;
    let raw = decompress_path(tmp.path(), tmpdir)?;
    let mut reader = BufReader::new(File::open(raw.path())?);
    cpio_extract(&mut reader, root)
}

// ---------------------------------------------------------------------------
// cpio (newc), used by rpm payloads
// ---------------------------------------------------------------------------

fn cpio_hex(field: &[u8]) -> u64 {
    let s = std::str::from_utf8(field).unwrap_or("0");
    u64::from_str_radix(s.trim(), 16).unwrap_or(0)
}

pub fn cpio_extract<R: Read>(reader: &mut R, root: &Path) -> Result<Vec<FsEntry>> {
    let mut out = Vec::new();
    loop {
        let mut h = [0u8; 110];
        if !read_full_or_eof(reader, &mut h)? {
            break;
        }
        if &h[0..6] != b"070701" && &h[0..6] != b"070702" {
            return err("cpio: unsupported header magic");
        }
        let mode = cpio_hex(&h[14..22]) as u32;
        let filesize = cpio_hex(&h[54..62]) as usize;
        if filesize > MAX_MEMBER {
            return err(format!("cpio: member size {filesize} exceeds the {MAX_MEMBER}-byte limit"));
        }
        let namesize = cpio_hex(&h[94..102]) as usize;
        if namesize == 0 || namesize > 65536 {
            return err("cpio: bad name size");
        }
        let mut namebuf = vec![0u8; namesize];
        reader.read_exact(&mut namebuf)?;
        skip_bytes(reader, (4 - ((110 + namesize) % 4)) % 4)?;
        let name = cstr_bytes(&namebuf);
        if name == "TRAILER!!!" {
            break;
        }
        let mut data = vec![0u8; filesize];
        if filesize > 0 {
            reader.read_exact(&mut data)?;
        }
        skip_bytes(reader, (4 - (filesize % 4)) % 4)?;

        let Some(rel) = safe_rel(&name) else {
            warn(&format!("skipping unsafe cpio path {name:?}"));
            continue;
        };
        let ft = mode & 0o170000;
        let kind = if ft == 0o040000 {
            '5'
        } else if ft == 0o120000 {
            '2'
        } else {
            '0'
        };
        let placed = place(root, &rel, kind, mode & 0o7777, &data, "")?;
        out.push(FsEntry {
            kind: placed,
            path: rel.to_string_lossy().to_string(),
            sha256: String::new(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// misc
// ---------------------------------------------------------------------------

/// Restore the default SIGPIPE behaviour so that `forst search x | head` exits
/// quietly instead of panicking on a broken pipe.
pub fn default_sigpipe() {
    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    unsafe {
        signal(SIGPIPE, SIG_DFL);
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&long),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn safe_rel_rejects_escapes() {
        assert_eq!(safe_rel("./usr/bin/sh").unwrap(), Path::new("usr/bin/sh"));
        assert!(safe_rel("../etc/passwd").is_none());
        assert!(safe_rel("/etc/passwd").is_none());
        assert!(safe_rel("usr/../../etc").is_none());
        assert!(safe_rel(".").is_none());
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KiB");
    }

    #[test]
    fn decompress_roundtrip_gzip() {
        // Compressed with the system gzip: forst delegates decompression to the
        // same tools it will find on a base system.
        use std::process::{Command, Stdio};
        let mut child = match Command::new("gzip")
            .arg("-c")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return, // no gzip on this host, nothing to prove
        };
        {
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(b"forsteri").unwrap();
        }
        let out = child.wait_with_output().unwrap();
        let back = decompress_bytes(&out.stdout).expect("gunzip");
        assert_eq!(String::from_utf8_lossy(&back), "forsteri");
    }

    #[test]
    fn oversized_tar_member_is_rejected_before_allocation() {
        let mut header = [0u8; 512];
        header[..1].copy_from_slice(b"x");
        header[100..108].copy_from_slice(b"0000644\0");
        // Octal 77777777777 is far beyond the 64 MiB buffering cap. A naive
        // parser would try to allocate it before noticing the file is short.
        header[124..136].copy_from_slice(b"77777777777\0");
        header[156] = b'0';
        let mut input = std::io::Cursor::new(header.to_vec());
        let result = tar_foreach(&mut input, &mut |_| Ok(()));
        assert!(result.is_err(), "oversized tar member must be rejected");
    }

    #[test]
    fn oversized_cpio_member_is_rejected_before_allocation() {
        let mut header = [0u8; 110];
        header[..6].copy_from_slice(b"070701");
        header[54..62].copy_from_slice(b"FFFFFFFF");
        header[94..102].copy_from_slice(b"00000002");
        let mut input = std::io::Cursor::new(header.to_vec());
        let root = std::env::temp_dir().join(format!("forst-cpio-{}", std::process::id()));
        let result = cpio_extract(&mut input, &root);
        assert!(result.is_err(), "oversized cpio member must be rejected");
    }
}
