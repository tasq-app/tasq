//! Update-availability check and `tasq update` subcommand.
//!
//! Two pieces:
//!
//! 1. [`run`] — handler for `tasq update`. Detects how tasq was installed
//!    (Homebrew, Cargo, Nix, plain binary). For a package manager it prints
//!    the command to run (we don't run `brew upgrade` behind the user's
//!    back). For a downloaded binary it updates itself: downloads the newest
//!    release for this platform, checks it against the release's `.sha256`,
//!    and swaps it in for the running executable (see [`self_update`]).
//!
//! 2. [`spawn_check`] — background thread invoked at TUI startup that consults
//!    a cache under `$XDG_CACHE_HOME/tasq/latest_version.json`. If the cache
//!    is missing or older than 24h, it shells out to `curl` to read the
//!    `tag_name` of the latest GitHub release, rewrites the cache, and returns
//!    the tag through an mpsc channel. The TUI's status bar reads it and
//!    appends an `↑ <version>` hint when newer than the running build. All
//!    failures are silent — a stale or missing cache simply means no hint.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How tasq appears to have been installed, judged by the path of the
/// currently-running executable. Used to recommend the right upgrade command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Homebrew,
    Cargo,
    Nix,
    Binary,
    Unknown,
}

/// Run the `tasq update` subcommand: detect the install method, then either
/// print the package manager's upgrade command or, for a downloaded binary,
/// update it in place. The caller (`main`) should `return Ok(())` after.
pub fn run() -> anyhow::Result<()> {
    cleanup_old_binary();
    let exe = std::env::current_exe().ok();
    let kind = exe
        .as_deref()
        .map(detect_kind)
        .unwrap_or(InstallKind::Unknown);
    let current = env!("CARGO_PKG_VERSION");
    println!("tasq {current}");
    if let Some(p) = &exe {
        println!("installed at: {}", p.display());
    }
    println!();
    match kind {
        InstallKind::Homebrew => {
            println!("Looks like a Homebrew install. Update with:");
            println!();
            println!("    brew update && brew upgrade tasq-app/tap/tasq");
        }
        InstallKind::Cargo => {
            println!("Looks like a `cargo install` build. Update with:");
            println!();
            println!("    cargo install --git https://github.com/tasq-app/tasq --force");
        }
        InstallKind::Nix => {
            println!("Looks like a Nix install. Update it through Nix, e.g.:");
            println!();
            println!("    nix profile upgrade tasq");
        }
        InstallKind::Binary => match &exe {
            Some(p) => {
                if let Err(e) = self_update(p, current) {
                    eprintln!("tasq: update failed: {e:#}");
                    eprintln!();
                    eprintln!("You can still update by hand: grab the latest from");
                    eprintln!("    {RELEASES_PAGE}");
                    eprintln!("and replace the file above.");
                    std::process::exit(1);
                }
            }
            None => println!("Grab the latest from {RELEASES_PAGE}"),
        },
        InstallKind::Unknown => {
            println!("Could not detect the install method. Options:");
            println!();
            println!("    brew upgrade tasq-app/tap/tasq");
            println!("    cargo install --git https://github.com/tasq-app/tasq --force");
            println!("    https://github.com/tasq-app/tasq/releases/latest");
        }
    }
    Ok(())
}

/// Classify an executable path into an [`InstallKind`]. Exposed for tests.
pub fn detect_kind(exe: &Path) -> InstallKind {
    let s = exe.to_string_lossy();
    if s.contains("/Cellar/")
        || s.starts_with("/opt/homebrew/")
        || s.starts_with("/usr/local/Homebrew/")
        || s.contains("/homebrew/Cellar/")
        || s.contains("/linuxbrew/")
    {
        return InstallKind::Homebrew;
    }
    if s.contains("/.cargo/bin/") || s.contains("\\.cargo\\bin\\") {
        return InstallKind::Cargo;
    }
    if s.starts_with("/nix/store/") {
        return InstallKind::Nix;
    }
    // A bare /usr/local/bin/tasq could be either a Homebrew shim (older
    // macOS) or a manual download. Without more signal, treat it as a binary.
    if !s.is_empty() {
        return InstallKind::Binary;
    }
    InstallKind::Unknown
}

const RELEASES_PAGE: &str = "https://github.com/tasq-app/tasq/releases/latest";
const DOWNLOAD_BASE: &str = "https://github.com/tasq-app/tasq/releases/download";
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;

/// The release target this build was compiled for, as named in the release
/// assets (`tasq-<tag>-<target>.<ext>`). `None` on platforms we don't ship.
pub fn release_target() -> Option<&'static str> {
    if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "aarch64",
        target_env = "gnu"
    )) {
        Some("aarch64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("x86_64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else {
        None
    }
}

/// The archive file name of a release asset for `target`.
pub fn asset_name(tag: &str, target: &str) -> String {
    let ext = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("tasq-{tag}-{target}.{ext}")
}

/// The hash out of a `.sha256` file: `shasum` writes `<hash>  <file>`, the
/// Windows build writes the bare (upper-case) hash. Returns it lower-cased,
/// or `None` when the first word isn't 64 hex digits. Exposed for tests.
pub fn parse_sha256_file(body: &str) -> Option<String> {
    let word = body.split_whitespace().next()?;
    if word.len() == 64 && word.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(word.to_ascii_lowercase())
    } else {
        None
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Download the newest release for this platform, verify it, and replace the
/// executable at `exe` with it.
fn self_update(exe: &Path, current: &str) -> anyhow::Result<()> {
    use anyhow::{Context, anyhow, bail};

    let target =
        release_target().ok_or_else(|| anyhow!("there is no release binary for this platform"))?;
    println!("Checking for the latest release...");
    let tag = fetch_latest_body()
        .and_then(|b| parse_tag_from_release_json(&b))
        .ok_or_else(|| anyhow!("couldn't read the latest release from GitHub (offline?)"))?;
    if !is_newer(&tag, current) {
        println!("Already up to date ({current}).");
        return Ok(());
    }
    println!("Updating {current} -> {}", tag.trim_start_matches('v'));

    let work = std::env::temp_dir().join(format!("tasq-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)
        .with_context(|| format!("couldn't create {}", work.display()))?;
    let result = (|| -> anyhow::Result<()> {
        let asset = asset_name(&tag, target);
        let url = format!("{DOWNLOAD_BASE}/{tag}/{asset}");
        let archive = work.join(&asset);
        let sums = work.join(format!("{asset}.sha256"));
        println!("Downloading {asset}...");
        download(&url, &archive)?;
        download(&format!("{url}.sha256"), &sums)?;

        let expected = parse_sha256_file(&std::fs::read_to_string(&sums)?)
            .ok_or_else(|| anyhow!("the release's .sha256 file is malformed"))?;
        let actual = sha256_hex(&std::fs::read(&archive)?);
        if actual != expected {
            bail!("checksum mismatch for {asset} (expected {expected}, got {actual})");
        }
        println!("Checksum OK.");

        let unpacked = work.join("unpacked");
        std::fs::create_dir_all(&unpacked)?;
        extract(&archive, &unpacked)?;
        let bin_name = if cfg!(windows) { "tasq.exe" } else { "tasq" };
        let new_bin = find_file(&unpacked, bin_name)
            .ok_or_else(|| anyhow!("{bin_name} not found in {asset}"))?;
        replace_exe(exe, &new_bin)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result?;

    // The new binary is in place: drop the "update available" hint.
    if let Some(p) = cache_path() {
        let _ = write_cache(&p, now_epoch(), &tag);
    }
    println!("Updated to {}.", tag.trim_start_matches('v'));
    Ok(())
}

fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "--retry",
            "2",
            "-m",
            &DOWNLOAD_TIMEOUT_SECS.to_string(),
            "-A",
            concat!("tasq/", env!("CARGO_PKG_VERSION")),
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .status()
        .context("couldn't run curl")?;
    if !status.success() {
        bail!("download failed: {url}");
    }
    Ok(())
}

/// Unpack a release archive. `tar` handles both: on Windows 10+ the bundled
/// `tar.exe` (bsdtar) reads zip files. We call it by its System32 path so a
/// GNU tar from Git Bash earlier on PATH (which can't) isn't picked up, and
/// fall back to PowerShell's Expand-Archive.
fn extract(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let tar = PathBuf::from(root).join("System32").join("tar.exe");
            if tar.is_file() {
                let ok = Command::new(&tar)
                    .arg("-xf")
                    .arg(archive)
                    .arg("-C")
                    .arg(dest)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    return Ok(());
                }
            }
        }
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "Expand-Archive -Force -LiteralPath '{}' -DestinationPath '{}'",
                archive.display(),
                dest.display()
            ))
            .status()
            .context("couldn't run powershell to unzip the release")?;
        if !status.success() {
            bail!("couldn't unzip {}", archive.display());
        }
        return Ok(());
    }
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status()
        .context("couldn't run tar")?;
    if !status.success() {
        bail!("couldn't unpack {}", archive.display());
    }
    Ok(())
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

/// Where a replaced Windows executable is parked until it can be deleted.
fn old_binary_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// Remove the executable a previous `tasq update` parked on Windows (it
/// couldn't be deleted while it was running). Silent and cheap.
pub fn cleanup_old_binary() {
    if cfg!(windows)
        && let Ok(exe) = std::env::current_exe()
    {
        let _ = std::fs::remove_file(old_binary_path(&exe));
    }
}

fn permission_hint(e: &io::Error, exe: &Path) -> String {
    if e.kind() == io::ErrorKind::PermissionDenied {
        if cfg!(windows) {
            format!(
                "no permission to write {} (run the terminal as administrator)",
                exe.display()
            )
        } else {
            format!(
                "no permission to write {} (try `sudo tasq update`)",
                exe.display()
            )
        }
    } else {
        format!("couldn't replace {}: {e}", exe.display())
    }
}

/// Swap `new_bin` in for the executable at `exe`.
///
/// Unix: copy next to `exe` and rename over it — atomic, and the running
/// process keeps its (now unlinked) inode. Windows: a running .exe can't be
/// overwritten or deleted, but it can be renamed, so park it as
/// `tasq.exe.old` (removed on the next run) and copy the new one in.
fn replace_exe(exe: &Path, new_bin: &Path) -> anyhow::Result<()> {
    use anyhow::anyhow;
    if cfg!(windows) {
        let old = old_binary_path(exe);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| anyhow!(permission_hint(&e, exe)))?;
        if let Err(e) = std::fs::copy(new_bin, exe) {
            // Put the old one back so tasq keeps working.
            let _ = std::fs::rename(&old, exe);
            return Err(anyhow!(permission_hint(&e, exe)));
        }
        let _ = std::fs::remove_file(&old);
        return Ok(());
    }
    let staged = exe.with_file_name(".tasq-update.new");
    std::fs::copy(new_bin, &staged).map_err(|e| anyhow!(permission_hint(&e, exe)))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }
    if let Err(e) = std::fs::rename(&staged, exe) {
        let _ = std::fs::remove_file(&staged);
        return Err(anyhow!(permission_hint(&e, exe)));
    }
    Ok(())
}

/// Spawn the background update check. Returns a receiver that yields exactly
/// one message — `Some(tag)` if a cached or freshly-fetched tag is available,
/// otherwise `None`. The receiver is dropped when the thread exits, so a
/// disconnect on `try_recv` means "give up, nothing's coming".
pub fn spawn_check() -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::sync_channel::<Option<String>>(1);
    thread::spawn(move || {
        let result = check_for_update();
        let _ = tx.send(result);
    });
    rx
}

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// How long to honor a cached *failure* (empty `tag`) before trying the
/// network again. Short enough to recover within an hour, long enough to
/// stop hammering GitHub once anonymous API calls have been rate-limited
/// (which is what burned us once during testing).
const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const CURL_TIMEOUT_SECS: u64 = 5;
/// The newest release, pre-releases included (`/releases/latest` skips
/// them, and tasq ships alphas and betas). The first `tag_name` in the list
/// is the newest.
const RELEASE_URL: &str = "https://api.github.com/repos/tasq-app/tasq/releases?per_page=1";

fn check_for_update() -> Option<String> {
    let cache_path = cache_path();
    let now = now_epoch();
    // Honor the cache for both success and failure. A non-empty tag within
    // CACHE_TTL is a success entry; an empty tag within NEGATIVE_CACHE_TTL
    // is a "we just tried and it failed, don't pummel GitHub again" marker.
    if let Some(p) = &cache_path
        && let Some((ts, tag)) = read_cache(p)
    {
        let age = now.saturating_sub(ts);
        if !tag.is_empty() && age < CACHE_TTL.as_secs() {
            return Some(tag);
        }
        if tag.is_empty() && age < NEGATIVE_CACHE_TTL.as_secs() {
            return None;
        }
    }
    // Cache is stale, missing, or expired-negative — try the network. Cache
    // either outcome so we don't retry on every launch when offline or
    // rate-limited.
    let tag = fetch_latest_body().and_then(|b| parse_tag_from_release_json(&b));
    if let Some(p) = &cache_path {
        let _ = write_cache(p, now, tag.as_deref().unwrap_or(""));
    }
    tag
}

fn fetch_latest_body() -> Option<String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "-m",
            &CURL_TIMEOUT_SECS.to_string(),
            "-H",
            "Accept: application/vnd.github+json",
            "-A",
            concat!("tasq/", env!("CARGO_PKG_VERSION")),
            RELEASE_URL,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Pull the `tag_name` value out of a GitHub release JSON payload. Doesn't
/// pull in a JSON parser — release payloads are well-formed enough that a
/// targeted string scan suffices, and a malformed payload simply returns
/// `None`. Exposed for unit testing.
pub fn parse_tag_from_release_json(body: &str) -> Option<String> {
    let key = "\"tag_name\"";
    let i = body.find(key)?;
    let rest = &body[i + key.len()..];
    let colon = rest.find(':')?;
    let after_colon = &rest[colon + 1..];
    let q = after_colon.find('"')?;
    let after_q = &after_colon[q + 1..];
    let end = after_q.find('"')?;
    let tag = &after_q[..end];
    if tag.is_empty() {
        return None;
    }
    Some(tag.to_string())
}

/// True when `latest` is a strictly newer version than `current`, by
/// semantic versioning: `x.y.z` compared numerically, then a pre-release
/// (`-alpha.2`, `-beta.1`, `-rc.1`) sorts before its release, and
/// pre-releases compare part by part (numbers numerically, words
/// alphabetically: alpha < beta < rc). A leading `v` is ignored.
pub fn is_newer(latest: &str, current: &str) -> bool {
    compare_versions(latest, current) == std::cmp::Ordering::Greater
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let split = |v: &str| -> (Vec<u64>, Option<String>) {
        let v = v.trim().trim_start_matches('v');
        let v = v.split('+').next().unwrap_or(v);
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (v, None),
        };
        let nums = core.split('.').map(|n| n.parse().unwrap_or(0)).collect();
        (nums, pre)
    };
    let (an, ap) = split(a);
    let (bn, bp) = split(b);
    for i in 0..an.len().max(bn.len()) {
        let (x, y) = (
            an.get(i).copied().unwrap_or(0),
            bn.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x.cmp(&y);
        }
    }
    match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let (mut xs, mut ys) = (x.split('.'), y.split('.'));
            loop {
                match (xs.next(), ys.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(p), Some(q)) => {
                        let ord = match (p.parse::<u64>(), q.parse::<u64>()) {
                            (Ok(m), Ok(n)) => m.cmp(&n),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => p.cmp(q),
                        };
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                }
            }
        }
    }
}

fn cache_path() -> Option<PathBuf> {
    let base = xdg_cache_home()?;
    Some(base.join("tasq").join("latest_version.json"))
}

fn xdg_cache_home() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("XDG_CACHE_HOME")
        && !v.is_empty()
    {
        let p = PathBuf::from(&v);
        if p.is_absolute() {
            return Some(p);
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".cache"))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read a cache file. Returns `(epoch_secs, tag)` on success. Format is a
/// tiny JSON-ish payload: `{"checked_at": <int>, "tag": "<str>"}`. An empty
/// `tag` is a sentinel for a cached failure (see [`NEGATIVE_CACHE_TTL`]);
/// callers distinguish success from failure on the returned string.
fn read_cache(path: &Path) -> Option<(u64, String)> {
    let body = std::fs::read_to_string(path).ok()?;
    let ts = scan_int(&body, "\"checked_at\"")?;
    let tag = scan_str(&body, "\"tag\"").unwrap_or_default();
    Some((ts, tag))
}

fn write_cache(path: &Path, now: u64, tag: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = format!("{{\"checked_at\": {now}, \"tag\": \"{tag}\"}}\n");
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn scan_int(body: &str, key: &str) -> Option<u64> {
    let i = body.find(key)?;
    let rest = &body[i + key.len()..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    let end = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

fn scan_str(body: &str, key: &str) -> Option<String> {
    let i = body.find(key)?;
    let rest = &body[i + key.len()..];
    let colon = rest.find(':')?;
    let after_colon = &rest[colon + 1..];
    let q = after_colon.find('"')?;
    let after_q = &after_colon[q + 1..];
    let end = after_q.find('"')?;
    Some(after_q[..end].to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detect_kind_homebrew_paths() {
        assert_eq!(
            detect_kind(&PathBuf::from("/opt/homebrew/bin/tasq")),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_kind(&PathBuf::from(
                "/opt/homebrew/Cellar/tasq/2026.5.3/bin/tasq"
            )),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_kind(&PathBuf::from("/usr/local/Cellar/tasq/2026.5.3/bin/tasq")),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_kind(&PathBuf::from(
                "/home/linuxbrew/.linuxbrew/Cellar/tasq/2026.5.3/bin/tasq"
            )),
            InstallKind::Homebrew
        );
    }

    #[test]
    fn detect_kind_cargo_path() {
        assert_eq!(
            detect_kind(&PathBuf::from("/home/m/.cargo/bin/tasq")),
            InstallKind::Cargo
        );
    }

    #[test]
    fn detect_kind_falls_back_to_binary() {
        assert_eq!(
            detect_kind(&PathBuf::from("/usr/local/bin/tasq")),
            InstallKind::Binary
        );
        assert_eq!(
            detect_kind(&PathBuf::from("/tmp/tasq")),
            InstallKind::Binary
        );
    }

    #[test]
    fn detect_kind_nix_path() {
        assert_eq!(
            detect_kind(&PathBuf::from("/nix/store/abc-tasq-0.9.1/bin/tasq")),
            InstallKind::Nix
        );
    }

    #[test]
    fn asset_names_match_the_release_workflow() {
        assert_eq!(
            asset_name("v0.9.2", "x86_64-pc-windows-msvc"),
            "tasq-v0.9.2-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            asset_name("v0.9.2", "aarch64-apple-darwin"),
            "tasq-v0.9.2-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn parse_sha256_file_handles_both_formats() {
        let h = "a".repeat(64);
        // shasum: `<hash>  <path>`
        assert_eq!(
            parse_sha256_file(&format!("{h}  dist/tasq.tar.gz\n")).as_deref(),
            Some(h.as_str())
        );
        // Windows Get-FileHash: bare upper-case hash, CRLF.
        assert_eq!(
            parse_sha256_file(&format!("{}\r\n", h.to_uppercase())).as_deref(),
            Some(h.as_str())
        );
        assert!(parse_sha256_file("not a hash").is_none());
        assert!(parse_sha256_file("").is_none());
    }

    #[test]
    fn sha256_hex_known_value() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn is_newer_follows_semver() {
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.3"), "numeric, not alphabetical");
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.0", "0.1.1"));
    }

    #[test]
    fn pre_releases_sort_before_their_release() {
        assert!(is_newer("0.1.0-alpha.2", "0.1.0-alpha.1"));
        assert!(is_newer("0.1.0-alpha.10", "0.1.0-alpha.9"));
        assert!(is_newer("0.1.0-beta.1", "0.1.0-alpha.7"));
        assert!(is_newer("0.1.0-rc.1", "0.1.0-beta.3"));
        assert!(is_newer("0.1.0", "0.1.0-rc.1"));
        assert!(!is_newer("0.1.0-alpha.1", "0.1.0"));
        assert!(is_newer("0.2.0-alpha.1", "0.1.0"));
    }

    #[test]
    fn is_newer_strips_v_prefix() {
        assert!(is_newer("v0.1.0-alpha.2", "0.1.0-alpha.1"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
    }

    #[test]
    fn parse_tag_extracts_first_tag_name() {
        let body = r#"{"url":"x","tag_name":"v2026.5.5","name":"2026.5.5"}"#;
        assert_eq!(
            parse_tag_from_release_json(body).as_deref(),
            Some("v2026.5.5")
        );
    }

    #[test]
    fn parse_tag_with_whitespace_and_extra_keys() {
        let body = r#"
        {
          "url": "x",
          "tag_name" : "2026.5.10" ,
          "draft": false
        }
        "#;
        assert_eq!(
            parse_tag_from_release_json(body).as_deref(),
            Some("2026.5.10")
        );
    }

    #[test]
    fn parse_tag_returns_none_on_missing_field() {
        let body = r#"{"name":"hi"}"#;
        assert!(parse_tag_from_release_json(body).is_none());
    }

    #[test]
    fn cache_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "tasq-update-cache-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest_version.json");
        write_cache(&path, 1_700_000_000, "2026.5.5").unwrap();
        let (ts, tag) = read_cache(&path).unwrap();
        assert_eq!(ts, 1_700_000_000);
        assert_eq!(tag, "2026.5.5");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_cache_none_on_missing_file() {
        assert!(read_cache(&PathBuf::from("/tmp/does-not-exist-xyzzy")).is_none());
    }

    #[test]
    fn cache_round_trip_empty_tag_is_negative_marker() {
        let dir = std::env::temp_dir().join(format!(
            "tasq-update-neg-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest_version.json");
        // A failed check writes an empty tag.
        write_cache(&path, 1_700_000_000, "").unwrap();
        let (ts, tag) = read_cache(&path).unwrap();
        assert_eq!(ts, 1_700_000_000);
        assert_eq!(tag, "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
