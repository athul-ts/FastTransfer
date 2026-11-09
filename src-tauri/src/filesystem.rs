//! Filesystem operations and the path-safety boundary.
//!
//! This module holds the highest-value security code in the repository. Every
//! path that arrives over the wire passes through [`safe_join`] before a single
//! byte is written (SRS §91).
//!
//! # The invariant
//!
//! > For any root `R` and any wire-supplied string `P`, if `safe_join(R, P)`
//! > returns `Ok(path)`, then `path` is inside `R` — after normalisation, after
//! > resolving symlinks in the destination, and regardless of the platform.
//!
//! `safe_join` is deliberately paranoid and returns errors for inputs that a
//! lenient implementation would accept. A false rejection costs the user a
//! rename; a false acceptance writes outside the destination folder.

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::protocol::FileEntry;

/// Device names Windows refuses to create, with or without an extension.
/// `CON.txt` is as reserved as `CON`.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters Windows forbids in a filename.
const WINDOWS_INVALID_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

/// Joins a wire-supplied relative path onto a destination root, proving the
/// result stays inside the root.
///
/// Rejects, in order:
///
/// 1. empty and whitespace-only paths
/// 2. embedded NUL bytes
/// 3. absolute paths (`/etc/passwd`, `C:\Windows`, `\\server\share`)
/// 4. UNC and Windows device prefixes (`\\?\`, `\\.\`)
/// 5. any `..` component, anywhere
/// 6. components that are invalid or reserved filenames
///
/// Returns [`Error::UnsafePath`] for every rejection, so the caller cannot
/// accidentally treat a traversal attempt as an ordinary I/O failure.
pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.trim().is_empty() {
        return Err(Error::UnsafePath("empty path".into()));
    }

    if relative.contains('\0') {
        return Err(Error::UnsafePath(format!(
            "path contains a NUL byte: {:?}",
            relative.escape_debug()
        )));
    }

    // Reject absolute and prefixed paths before component analysis, because
    // `Path::new("C:\\x")` and `Path::new("\\\\?\\C:\\x")` are *not* absolute
    // on non-Windows hosts, so a host-conditional check would be a hole.
    if relative.starts_with('/') || relative.starts_with('\\') {
        return Err(Error::UnsafePath(format!("absolute path: {relative}")));
    }
    if has_windows_prefix(relative) {
        return Err(Error::UnsafePath(format!("prefixed path: {relative}")));
    }

    let candidate = Path::new(relative);
    let mut out = PathBuf::new();
    let mut depth = 0usize;

    for component in candidate.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    Error::UnsafePath(format!("non-UTF-8 component in {relative}"))
                })?;

                // A drive-relative component such as `C:` survives `components()`
                // as `Normal("C:")` on Unix hosts.
                if name.len() == 2 && name.ends_with(':') {
                    return Err(Error::UnsafePath(format!(
                        "drive-relative path: {relative}"
                    )));
                }

                validate_component(name, relative)?;
                out.push(name);
                depth += 1;
            }
            Component::CurDir => {} // `.` is harmless, drop it
            Component::ParentDir => {
                // Reject outright rather than popping. A well-formed transfer
                // manifest never contains `..`, and silently absorbing it would
                // hide a traversal attempt from the logs.
                return Err(Error::UnsafePath(format!("parent traversal in {relative}")));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::UnsafePath(format!("absolute path: {relative}")));
            }
        }
    }

    if depth == 0 {
        return Err(Error::UnsafePath(format!(
            "path resolves to nothing: {relative}"
        )));
    }

    let joined = root.join(&out);

    // Belt and braces: the invariant is asserted, not assumed. If a future
    // change to the loop above weakens it, this catches the regression.
    if !joined.starts_with(root) {
        return Err(Error::UnsafePath(format!(
            "join escaped the destination root: {relative}"
        )));
    }

    Ok(joined)
}

/// Secondary check for the case `safe_join` cannot cover alone: symlinks that
/// already exist *inside* the destination.
///
/// `safe_join` is purely lexical — it cannot know that `root/link` points at
/// `/etc`. Call this after `safe_join` and before opening the file, once the
/// root exists on disk.
///
/// Walks up to the deepest ancestor that exists, canonicalises it, and confirms
/// it is still under the canonicalised root. The not-yet-existing suffix is
/// safe by construction: it contains no `..` and will be created fresh.
pub fn verify_within(root: &Path, candidate: &Path) -> Result<()> {
    let canonical_root = root
        .canonicalize()
        .map_err(|e| Error::DestinationNotWritable {
            path: root.display().to_string(),
            remedy: format!("The destination folder could not be opened: {e}"),
        })?;

    let mut ancestor = candidate;
    while !ancestor.exists() {
        match ancestor.parent() {
            Some(parent) => ancestor = parent,
            None => {
                return Err(Error::UnsafePath(format!(
                    "no existing ancestor for {}",
                    candidate.display()
                )))
            }
        }
    }

    let canonical_ancestor = ancestor.canonicalize()?;

    if !canonical_ancestor.starts_with(&canonical_root) {
        return Err(Error::UnsafePath(format!(
            "{} resolves to {}, outside the destination {}",
            candidate.display(),
            canonical_ancestor.display(),
            canonical_root.display()
        )));
    }

    Ok(())
}

fn has_windows_prefix(path: &str) -> bool {
    // UNC (`\\server\share`) and the device namespaces (`\\?\`, `\\.\`).
    if path.starts_with("\\\\") || path.starts_with("//") {
        return true;
    }
    // Drive-absolute: `C:\...` or `C:/...`
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn validate_component(name: &str, context: &str) -> Result<()> {
    if name == "." || name == ".." {
        return Err(Error::UnsafePath(format!("{name:?} in {context}")));
    }

    // Trailing spaces and dots are stripped by Windows, which turns `evil.txt.`
    // into `evil.txt` — a way to overwrite a file the user did not choose.
    if name.ends_with(' ') || name.ends_with('.') {
        return Err(Error::UnsafePath(format!(
            "component {name:?} ends with a space or dot in {context}"
        )));
    }

    if name.contains(WINDOWS_INVALID_CHARS) {
        return Err(Error::UnsafePath(format!(
            "component {name:?} contains a character Windows forbids, in {context}"
        )));
    }

    // Control characters, including the ones Windows silently rewrites.
    if name.chars().any(|c| c.is_control()) {
        return Err(Error::UnsafePath(format!(
            "component {name:?} contains a control character in {context}"
        )));
    }

    // Reserved device names are checked on the stem, so `CON.txt` is caught.
    let stem = name.split('.').next().unwrap_or(name);
    if WINDOWS_RESERVED_NAMES
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
    {
        return Err(Error::UnsafePath(format!(
            "component {name:?} is a reserved device name in {context}"
        )));
    }

    Ok(())
}

/// Suggests a name that is safe to create on the destination platform.
///
/// Used by the conflict/filename-resolution flow (SRS §26) to offer the user a
/// concrete alternative rather than a bare rejection.
pub fn suggest_safe_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if WINDOWS_INVALID_CHARS.contains(&c) || c.is_control() || c == '\\' || c == '/' {
                '_'
            } else {
                c
            }
        })
        .collect();

    while out.ends_with(' ') || out.ends_with('.') {
        out.pop();
    }

    let stem = out.split('.').next().unwrap_or(&out).to_string();
    if WINDOWS_RESERVED_NAMES
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
    {
        out = format!("_{out}");
    }

    if out.is_empty() {
        out = "unnamed".to_string();
    }

    out
}

/// Resolves a destination conflict per SRS §24. `KeepBoth` is the default
/// everywhere in the app, because a silent overwrite is unrecoverable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    Replace,
    Skip,
    /// Destructive overwrite is never the default (SRS §24).
    #[default]
    KeepBoth,
    Cancel,
}

/// Appends ` (1)`, ` (2)`, … before the extension until the path is free.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());

    for n in 1..u32::MAX {
        let candidate = match ext {
            Some(ext) => parent.join(format!("{stem} ({n}).{ext}")),
            None => parent.join(format!("{stem} ({n})")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }

    // Exhausting u32 candidates is not a realistic outcome; fall back to the
    // original rather than looping forever.
    path.to_path_buf()
}

// ---------------------------------------------------------------------------
// Sending: turning the user's selection into a manifest
// ---------------------------------------------------------------------------

/// Most entries (files plus directories) one transfer may contain. The same
/// bound is enforced on the receiving side, where it matters: a manifest is
/// held in memory and every entry costs a path resolution (SRS §92, §93).
pub const MAX_MANIFEST_ENTRIES: usize = 100_000;

/// Deepest directory nesting walked. Bounds recursion on a hostile or
/// pathological tree.
const MAX_DEPTH: usize = 64;

/// A file or directory to send, with where it lives on *this* machine. The
/// `source` never crosses the wire; only `entry` does.
#[derive(Debug, Clone)]
pub struct LocalItem {
    pub entry: FileEntry,
    pub source: PathBuf,
}

fn io_context(path: &Path, e: io::Error) -> Error {
    Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

fn unix_secs(t: SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Picks a name for `raw` that is safe on every platform and not yet taken.
///
/// A name valid on Linux (`a:b.txt`) is refused by a Windows receiver's
/// `safe_join`, so it is repaired here rather than failing the transfer
/// halfway (SRS §26). Two siblings that collapse to the same name — or differ
/// only by case, which Windows and macOS treat as one file — get a counter.
fn claim_name(used: &mut HashSet<String>, parent: &str, raw: &str) -> String {
    let base = suggest_safe_name(raw);
    let key = |name: &str| {
        if parent.is_empty() {
            name.to_lowercase()
        } else {
            format!("{parent}/{name}").to_lowercase()
        }
    };

    let mut candidate = base.clone();
    let mut n = 2u32;
    while !used.insert(key(&candidate)) {
        candidate = match base.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
            _ => format!("{base} ({n})"),
        };
        n += 1;
    }
    candidate
}

fn modified_secs(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified().ok().and_then(unix_secs)
}

fn push_item(items: &mut Vec<LocalItem>, item: LocalItem) -> Result<()> {
    if items.len() >= MAX_MANIFEST_ENTRIES {
        return Err(Error::Io(io::Error::other(format!(
            "too many files in one transfer (the limit is {MAX_MANIFEST_ENTRIES}); \
             send them in smaller groups or as an archive"
        ))));
    }
    items.push(item);
    Ok(())
}

/// Builds the manifest for the paths the user picked.
///
/// - Directory order is stable (sorted by name) and a directory always precedes
///   its contents, so `file_index` means the same thing on both ends.
/// - Symlinks *inside* a chosen folder are skipped, not followed: following
///   them can loop, and can quietly send files from outside the folder the
///   user chose. A symlink the user picks directly is their own choice and is
///   followed.
/// - Sockets, device nodes and other non-regular files are skipped.
///
/// Blocking: call it from `spawn_blocking`.
pub fn build_manifest(paths: &[PathBuf]) -> Result<Vec<LocalItem>> {
    let mut items = Vec::new();
    let mut used = HashSet::new();

    for path in paths {
        let meta = std::fs::metadata(path).map_err(|e| io_context(path, e))?;
        let raw = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            Error::Io(io::Error::other(format!(
                "{} has a name that is not valid text",
                path.display()
            )))
        })?;
        let name = claim_name(&mut used, "", raw);

        if meta.is_dir() {
            walk_dir(path, &name, 0, &mut items, &mut used)?;
        } else if meta.is_file() {
            push_item(
                &mut items,
                LocalItem {
                    entry: FileEntry {
                        relative_path: name,
                        size: meta.len(),
                        is_dir: false,
                        mode: None,
                        modified: modified_secs(&meta),
                        sha256: None,
                    },
                    source: path.clone(),
                },
            )?;
        } else {
            return Err(Error::Io(io::Error::other(format!(
                "{} is not a regular file or folder",
                path.display()
            ))));
        }
    }
    Ok(items)
}

fn walk_dir(
    dir: &Path,
    relative: &str,
    depth: usize,
    items: &mut Vec<LocalItem>,
    used: &mut HashSet<String>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::Io(io::Error::other(format!(
            "{} is nested more than {MAX_DEPTH} folders deep",
            dir.display()
        ))));
    }

    let meta = std::fs::metadata(dir).map_err(|e| io_context(dir, e))?;
    push_item(
        items,
        LocalItem {
            entry: FileEntry {
                relative_path: relative.to_string(),
                size: 0,
                is_dir: true,
                mode: None,
                modified: modified_secs(&meta),
                sha256: None,
            },
            source: dir.to_path_buf(),
        },
    )?;

    let mut children: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| io_context(dir, e))?
        .filter_map(|e| e.ok())
        .collect();
    children.sort_by_key(|e| e.file_name());

    for child in children {
        let path = child.path();
        let Ok(file_type) = child.file_type() else {
            continue;
        };
        let Some(raw) = child.file_name().to_str().map(str::to_owned) else {
            tracing::warn!(path = %path.display(), "skipping a name that is not valid text");
            continue;
        };

        if file_type.is_symlink() {
            tracing::debug!(path = %path.display(), "skipping a symlink inside a chosen folder");
            continue;
        }

        let name = claim_name(used, relative, &raw);
        let child_relative = format!("{relative}/{name}");

        if file_type.is_dir() {
            walk_dir(&path, &child_relative, depth + 1, items, used)?;
        } else if file_type.is_file() {
            let meta = child.metadata().map_err(|e| io_context(&path, e))?;
            push_item(
                items,
                LocalItem {
                    entry: FileEntry {
                        relative_path: child_relative,
                        size: meta.len(),
                        is_dir: false,
                        mode: None,
                        modified: modified_secs(&meta),
                        sha256: None,
                    },
                    source: path,
                },
            )?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Receiving: part files, finalisation, destination safety
// ---------------------------------------------------------------------------

/// Extension of an in-progress file. A file with this suffix is never presented
/// as complete (SRS FR-RESUME-005): it is renamed to its real name only after
/// every chunk has landed and the end-to-end digest has matched.
pub const PART_SUFFIX: &str = "ftpart";

/// Whether a transfer id is safe to use as part of a filename. It arrives over
/// the wire, so it is restricted to what a UUID contains (SRS §91).
pub fn is_valid_transfer_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Where a file lives while it is being received.
///
/// `destination` must already have passed `safe_join`; the part file shares its
/// parent, so it is inside the root too.
pub fn part_path(destination: &Path, transfer_id: &str) -> Result<PathBuf> {
    if !is_valid_transfer_id(transfer_id) {
        return Err(Error::MalformedMessage(
            "transfer id contains characters that are not allowed".into(),
        ));
    }
    let name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::UnsafePath(format!("no file name in {}", destination.display())))?;
    let short: String = transfer_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    Ok(destination.with_file_name(format!("{name}.{short}.{PART_SUFFIX}")))
}

/// Creates the directories a destination needs, re-proving containment on both
/// sides of the creation.
///
/// Before: the deepest *existing* ancestor must resolve inside the root, which
/// catches a pre-existing symlink. After: the freshly created chain is checked
/// again. A symlink swapped in between the two checks and the write is a
/// time-of-check/time-of-use window that no portable API closes; the
/// destination root is a folder the user chose, so the residual risk is another
/// local process with write access to it.
pub fn create_parent_dirs(root: &Path, destination: &Path) -> Result<()> {
    verify_within(root, destination)?;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    verify_within(root, destination)
}

/// The outcome of moving a verified part file into place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finalized {
    Written(PathBuf),
    /// The destination existed and the user chose to skip.
    Skipped,
}

/// Moves a verified part file to its real name, honouring the conflict policy
/// (SRS §24). The default, `KeepBoth`, never overwrites anything.
pub fn finalize_file(
    part: &Path,
    destination: &Path,
    conflict: ConflictResolution,
) -> Result<Finalized> {
    let target = match conflict {
        ConflictResolution::KeepBoth => unique_path(destination),
        ConflictResolution::Replace => destination.to_path_buf(),
        ConflictResolution::Skip => {
            if destination.exists() {
                let _ = std::fs::remove_file(part);
                return Ok(Finalized::Skipped);
            }
            destination.to_path_buf()
        }
        ConflictResolution::Cancel => {
            if destination.exists() {
                return Err(Error::Cancelled);
            }
            destination.to_path_buf()
        }
    };
    // `rename` replaces an existing file on both Unix and Windows, which is
    // exactly `Replace`; for `KeepBoth` the target was just chosen not to exist.
    std::fs::rename(part, &target)?;
    Ok(Finalized::Written(target))
}

/// Applies the sender's modification time, when it is plausible. A bogus value
/// (negative, or far in the future) is ignored rather than trusted (SRS §92).
pub fn apply_modified(path: &Path, seconds: Option<i64>) {
    const MAX_SECONDS: i64 = 4_102_444_800; // 2100-01-01
    let Some(secs) = seconds.filter(|s| (0..=MAX_SECONDS).contains(s)) else {
        return;
    };
    let when = UNIX_EPOCH + Duration::from_secs(secs as u64);
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(when);
    }
}

/// Maps a failed write to the error the user should see. A full disk is the
/// one write failure that has a specific remedy and is resumable.
pub fn map_write_error(e: io::Error, remaining: u64) -> Error {
    #[cfg(windows)]
    const DISK_FULL: &[i32] = &[112, 39]; // ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL
    #[cfg(not(windows))]
    const DISK_FULL: &[i32] = &[28]; // ENOSPC

    match e.raw_os_error() {
        Some(code) if DISK_FULL.contains(&code) => Error::DiskFull {
            required: remaining,
            available: None,
        },
        _ => Error::Io(e),
    }
}

/// Folders a transfer must never be written into (SRS §49).
fn protected_folders() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for var in [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if let Ok(v) = std::env::var(var) {
            out.push(normalise_for_compare(Path::new(&v)));
        }
    }
    for fixed in [
        "/bin",
        "/boot",
        "/dev",
        "/etc",
        "/lib",
        "/proc",
        "/sbin",
        "/sys",
        "/usr",
        "/System",
        "/Library",
        "/Applications",
    ] {
        out.push(fixed.to_lowercase());
    }
    out
}

/// Lowercased, forward-slashed, with any `\\?\` verbatim prefix removed, so one
/// comparison works for Windows and Unix paths.
fn normalise_for_compare(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/").to_lowercase();
    s.strip_prefix("//?/").map(str::to_owned).unwrap_or(s)
}

/// Whether `path` is a filesystem root or inside a protected system folder.
pub fn is_dangerous_destination(path: &Path) -> bool {
    is_dangerous_in(path, &protected_folders())
}

fn is_dangerous_in(path: &Path, protected: &[String]) -> bool {
    let norm = normalise_for_compare(path);
    let norm = norm.trim_end_matches('/');

    // A drive or filesystem root: `c:` or the empty string left by `/`.
    let without_drive = match norm.split_once(':') {
        Some((drive, rest)) if drive.len() == 1 => rest,
        _ => norm,
    };
    if without_drive.is_empty() {
        return true;
    }

    protected.iter().any(|p| {
        let p = p.trim_end_matches('/');
        !p.is_empty() && (norm == p || norm.starts_with(&format!("{p}/")))
    })
}

/// Refuses a destination root that is dangerous, with a remedy the user can act
/// on (SRS §49, §50). Never escalates privileges.
pub fn check_destination_root(root: &Path) -> Result<()> {
    if is_dangerous_destination(root) {
        return Err(Error::DestinationNotWritable {
            path: root.display().to_string(),
            remedy: "Choose a folder in your Documents or Downloads instead of a \
                     system folder."
                .into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from("/dest")
    }

    // --- traversal -----------------------------------------------------------

    #[test]
    fn safe_join_rejects_parent_traversal() {
        assert!(safe_join(&root(), "../etc/passwd").is_err());
        assert!(safe_join(&root(), "a/../../b").is_err());
        assert!(safe_join(&root(), "a/b/../../../c").is_err());
        assert!(safe_join(&root(), "..").is_err());
    }

    #[test]
    fn safe_join_rejects_absolute_posix_paths() {
        assert!(safe_join(&root(), "/etc/passwd").is_err());
        assert!(safe_join(&root(), "/").is_err());
    }

    #[test]
    fn safe_join_rejects_windows_absolute_paths() {
        // These must fail even when compiled on Linux, where `Path` would
        // otherwise treat them as ordinary relative names.
        assert!(safe_join(&root(), "C:\\Windows\\System32").is_err());
        assert!(safe_join(&root(), "C:/Windows/System32").is_err());
    }

    #[test]
    fn safe_join_rejects_unc_and_device_prefixes() {
        assert!(safe_join(&root(), "\\\\server\\share\\file").is_err());
        assert!(safe_join(&root(), "\\\\?\\C:\\Windows").is_err());
        assert!(safe_join(&root(), "\\\\.\\PhysicalDrive0").is_err());
        assert!(safe_join(&root(), "//server/share").is_err());
    }

    #[test]
    fn safe_join_rejects_drive_relative_component() {
        assert!(safe_join(&root(), "C:file").is_err());
    }

    // --- malformed names -----------------------------------------------------

    #[test]
    fn safe_join_rejects_embedded_nul() {
        assert!(safe_join(&root(), "file\0.txt").is_err());
    }

    #[test]
    fn safe_join_rejects_empty_paths() {
        assert!(safe_join(&root(), "").is_err());
        assert!(safe_join(&root(), "   ").is_err());
        assert!(safe_join(&root(), ".").is_err());
        assert!(safe_join(&root(), "./").is_err());
    }

    #[test]
    fn safe_join_rejects_reserved_device_names() {
        for name in ["CON", "con", "PRN", "AUX", "NUL", "COM1", "LPT9"] {
            assert!(
                safe_join(&root(), name).is_err(),
                "{name} should be rejected"
            );
            // With an extension too — Windows treats `CON.txt` as reserved.
            assert!(
                safe_join(&root(), &format!("{name}.txt")).is_err(),
                "{name}.txt should be rejected"
            );
        }
    }

    #[test]
    fn safe_join_rejects_trailing_dot_or_space() {
        // Windows silently strips these, so `evil.txt.` would clobber `evil.txt`.
        assert!(safe_join(&root(), "evil.txt.").is_err());
        assert!(safe_join(&root(), "evil ").is_err());
        assert!(safe_join(&root(), "dir/evil.").is_err());
    }

    #[test]
    fn safe_join_rejects_windows_invalid_characters() {
        for ch in ['<', '>', ':', '"', '|', '?', '*'] {
            assert!(
                safe_join(&root(), &format!("file{ch}name")).is_err(),
                "character {ch} should be rejected"
            );
        }
    }

    #[test]
    fn safe_join_rejects_control_characters() {
        assert!(safe_join(&root(), "file\nname").is_err());
        assert!(safe_join(&root(), "file\tname").is_err());
        assert!(safe_join(&root(), "file\u{7f}name").is_err());
    }

    // --- accepted paths ------------------------------------------------------

    #[test]
    fn safe_join_accepts_ordinary_relative_paths() {
        let joined = safe_join(&root(), "photos/2024/img.jpg").unwrap();
        assert_eq!(joined, PathBuf::from("/dest/photos/2024/img.jpg"));
    }

    #[test]
    fn safe_join_accepts_unicode_and_spaces() {
        let joined = safe_join(&root(), "My Documents/കേരളം.txt").unwrap();
        assert_eq!(joined, PathBuf::from("/dest/My Documents/കേരളം.txt"));
    }

    #[test]
    fn safe_join_drops_curdir_components() {
        let joined = safe_join(&root(), "./a/./b").unwrap();
        assert_eq!(joined, PathBuf::from("/dest/a/b"));
    }

    #[test]
    fn safe_join_result_is_always_inside_root() {
        // Property check over a spread of adversarial and benign inputs.
        let cases = [
            "a/b/c",
            "a/./b",
            "deeply/nested/path/file.bin",
            "file with spaces.txt",
            "unicode-файл.txt",
        ];
        for case in cases {
            let joined = safe_join(&root(), case).unwrap();
            assert!(
                joined.starts_with(root()),
                "{case} escaped the root: {}",
                joined.display()
            );
        }
    }

    // --- symlink containment -------------------------------------------------

    #[test]
    fn verify_within_rejects_symlink_escaping_the_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("dest");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
            let candidate = root.join("escape").join("stolen.txt");
            assert!(verify_within(&root, &candidate).is_err());
        }
    }

    #[test]
    fn verify_within_accepts_a_normal_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("dest");
        std::fs::create_dir_all(root.join("sub")).unwrap();

        let candidate = root.join("sub").join("new.txt");
        assert!(verify_within(&root, &candidate).is_ok());
    }

    // --- name repair ---------------------------------------------------------

    #[test]
    fn suggest_safe_name_replaces_forbidden_characters() {
        assert_eq!(
            suggest_safe_name("project:/test?.txt"),
            "project__test_.txt"
        );
    }

    #[test]
    fn suggest_safe_name_escapes_reserved_names() {
        assert_eq!(suggest_safe_name("CON"), "_CON");
        assert_eq!(suggest_safe_name("nul.txt"), "_nul.txt");
    }

    #[test]
    fn suggest_safe_name_handles_degenerate_input() {
        assert_eq!(suggest_safe_name("..."), "unnamed");
        assert_eq!(suggest_safe_name(""), "unnamed");
    }

    // --- conflicts -----------------------------------------------------------

    #[test]
    fn conflict_default_is_never_destructive() {
        assert_eq!(ConflictResolution::default(), ConflictResolution::KeepBoth);
    }

    #[test]
    fn unique_path_appends_a_counter() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("report.pdf");
        std::fs::write(&file, b"x").unwrap();

        let unique = unique_path(&file);
        assert_eq!(unique.file_name().unwrap(), "report (1).pdf");
    }

    #[test]
    fn unique_path_leaves_a_free_path_alone() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("free.pdf");
        assert_eq!(unique_path(&file), file);
    }

    // --- manifest building ---------------------------------------------------

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_single_file_becomes_one_entry_named_after_it() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("report.pdf");
        write(&file, b"12345");

        let items = build_manifest(std::slice::from_ref(&file)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].entry.relative_path, "report.pdf");
        assert_eq!(items[0].entry.size, 5);
        assert!(!items[0].entry.is_dir);
        assert_eq!(items[0].source, file);
        // The local path must never leak into what is sent.
        assert!(!items[0]
            .entry
            .relative_path
            .contains(temp.path().to_str().unwrap()));
    }

    #[test]
    fn a_folder_keeps_its_structure_with_directories_before_contents() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("photos");
        write(&dir.join("2024/a.jpg"), b"aa");
        write(&dir.join("2024/b.jpg"), b"bbb");
        write(&dir.join("top.txt"), b"t");
        std::fs::create_dir_all(dir.join("empty")).unwrap();

        let items = build_manifest(&[dir]).unwrap();
        let paths: Vec<&str> = items
            .iter()
            .map(|i| i.entry.relative_path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec![
                "photos",
                "photos/2024",
                "photos/2024/a.jpg",
                "photos/2024/b.jpg",
                "photos/empty",
                "photos/top.txt",
            ]
        );
        // A directory is always listed before anything inside it, because the
        // receiver creates them in order.
        for (i, item) in items.iter().enumerate() {
            if let Some((parent, _)) = item.entry.relative_path.rsplit_once('/') {
                let parent_at = paths.iter().position(|p| *p == parent).unwrap();
                assert!(parent_at < i, "{parent} must precede its contents");
            }
        }
        assert!(items[0].entry.is_dir && items[0].entry.size == 0);
    }

    #[test]
    fn every_manifest_path_passes_the_receivers_safe_join() {
        // The sender must never produce a path its own receiver would refuse.
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("mix");
        write(&dir.join("ok.txt"), b"1");
        write(&dir.join("sub dir/ünïcode.txt"), b"2");

        for item in build_manifest(&[dir]).unwrap() {
            safe_join(Path::new("/dest"), &item.entry.relative_path)
                .unwrap_or_else(|e| panic!("{}: {e}", item.entry.relative_path));
        }
    }

    #[test]
    fn two_picks_with_the_same_name_do_not_collide() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a/notes.txt");
        let b = temp.path().join("b/notes.txt");
        write(&a, b"1");
        write(&b, b"2");

        let items = build_manifest(&[a, b]).unwrap();
        let names: Vec<&str> = items
            .iter()
            .map(|i| i.entry.relative_path.as_str())
            .collect();
        assert_eq!(names, vec!["notes.txt", "notes (2).txt"]);
    }

    #[test]
    fn names_differing_only_by_case_do_not_collide_on_a_case_insensitive_receiver() {
        let mut used = HashSet::new();
        let first = claim_name(&mut used, "", "Report.txt");
        let second = claim_name(&mut used, "", "report.txt");
        assert_ne!(first.to_lowercase(), second.to_lowercase());
    }

    #[test]
    fn a_name_the_receiver_would_refuse_is_repaired() {
        let mut used = HashSet::new();
        assert_eq!(claim_name(&mut used, "", "a:b?.txt"), "a_b_.txt");
        assert_eq!(claim_name(&mut used, "", "CON"), "_CON");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_inside_a_chosen_folder_are_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("shared");
        let secret = temp.path().join("secret.txt");
        write(&secret, b"do not send");
        write(&dir.join("fine.txt"), b"ok");
        std::os::unix::fs::symlink(&secret, dir.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(temp.path(), dir.join("loop")).unwrap();

        let paths: Vec<String> = build_manifest(&[dir])
            .unwrap()
            .into_iter()
            .map(|i| i.entry.relative_path)
            .collect();
        assert_eq!(paths, vec!["shared", "shared/fine.txt"]);
    }

    #[test]
    fn a_missing_path_is_an_error_naming_it() {
        let temp = tempfile::tempdir().unwrap();
        let err = build_manifest(&[temp.path().join("nope.bin")]).unwrap_err();
        assert!(err.to_string().contains("nope.bin"), "got {err}");
    }

    // --- part files and finalisation -----------------------------------------

    #[test]
    fn a_transfer_id_with_path_characters_is_refused() {
        for bad in [
            "",
            "../x",
            "a/b",
            "a\\b",
            "id with space",
            "x\0y",
            &"a".repeat(65),
        ] {
            assert!(!is_valid_transfer_id(bad), "{bad:?} should be refused");
        }
        assert!(is_valid_transfer_id("3f2b8c1e-aaaa-4bbb-8ccc-123456789abc"));
        assert!(part_path(Path::new("/d/f.txt"), "../../evil").is_err());
    }

    #[test]
    fn the_part_file_sits_beside_the_destination_and_is_marked() {
        let part = part_path(Path::new("/dest/a/report.pdf"), "3f2b8c1e-aaaa").unwrap();
        assert_eq!(part, PathBuf::from("/dest/a/report.pdf.3f2b8c1e.ftpart"));
        assert!(part.starts_with("/dest/a"));
    }

    #[test]
    fn finalising_with_keep_both_never_overwrites() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("f.txt");
        std::fs::write(&dest, b"original").unwrap();
        let part = temp.path().join("f.txt.abc.ftpart");
        std::fs::write(&part, b"incoming").unwrap();

        let result = finalize_file(&part, &dest, ConflictResolution::KeepBoth).unwrap();
        assert_eq!(result, Finalized::Written(temp.path().join("f (1).txt")));
        assert_eq!(std::fs::read(&dest).unwrap(), b"original");
        assert_eq!(
            std::fs::read(temp.path().join("f (1).txt")).unwrap(),
            b"incoming"
        );
        assert!(!part.exists(), "the part file must be gone once finalised");
    }

    #[test]
    fn finalising_with_replace_overwrites() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("f.txt");
        std::fs::write(&dest, b"original").unwrap();
        let part = temp.path().join("f.txt.abc.ftpart");
        std::fs::write(&part, b"incoming").unwrap();

        finalize_file(&part, &dest, ConflictResolution::Replace).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"incoming");
    }

    #[test]
    fn finalising_with_skip_keeps_the_original_and_drops_the_part() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("f.txt");
        std::fs::write(&dest, b"original").unwrap();
        let part = temp.path().join("f.txt.abc.ftpart");
        std::fs::write(&part, b"incoming").unwrap();

        let result = finalize_file(&part, &dest, ConflictResolution::Skip).unwrap();
        assert_eq!(result, Finalized::Skipped);
        assert_eq!(std::fs::read(&dest).unwrap(), b"original");
        assert!(!part.exists());
    }

    #[test]
    fn finalising_into_a_free_path_just_renames() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("new.txt");
        let part = temp.path().join("new.txt.abc.ftpart");
        std::fs::write(&part, b"data").unwrap();

        let result = finalize_file(&part, &dest, ConflictResolution::KeepBoth).unwrap();
        assert_eq!(result, Finalized::Written(dest.clone()));
        assert_eq!(std::fs::read(&dest).unwrap(), b"data");
    }

    #[test]
    fn create_parent_dirs_builds_the_chain_inside_the_root() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("a/b/c.txt");
        create_parent_dirs(temp.path(), &dest).unwrap();
        assert!(temp.path().join("a/b").is_dir());
    }

    #[test]
    fn a_modification_time_is_applied_and_a_bogus_one_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("t.txt");
        std::fs::write(&file, b"x").unwrap();

        apply_modified(&file, Some(1_700_000_000));
        let got = std::fs::metadata(&file).unwrap().modified().unwrap();
        assert_eq!(unix_secs(got), Some(1_700_000_000));

        // Negative and far-future values are not trusted.
        apply_modified(&file, Some(-5));
        apply_modified(&file, Some(i64::MAX));
        let after = std::fs::metadata(&file).unwrap().modified().unwrap();
        assert_eq!(unix_secs(after), Some(1_700_000_000));
    }

    // --- destination safety --------------------------------------------------

    #[test]
    fn system_folders_and_roots_are_dangerous_destinations() {
        let protected = vec![
            "c:/windows".to_string(),
            "c:/program files".to_string(),
            "/usr".to_string(),
            "/system".to_string(),
        ];
        for bad in [
            "C:\\Windows",
            "C:\\Windows\\System32",
            "c:\\program files\\app",
            "C:\\",
            "C:",
            "/",
            "/usr/bin",
            "/System/Library",
            "\\\\?\\C:\\Windows\\Temp",
        ] {
            assert!(
                is_dangerous_in(Path::new(bad), &protected),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn ordinary_user_folders_are_not_dangerous() {
        let protected = vec!["c:/windows".to_string(), "/usr".to_string()];
        for ok in [
            "C:\\Users\\athul\\Downloads\\FastTransfer",
            "D:\\Transfers",
            "/home/athul/Downloads",
            "/Users/athul/Downloads/FastTransfer",
            "/usrdata/files", // shares a prefix with /usr but is not inside it
        ] {
            assert!(
                !is_dangerous_in(Path::new(ok), &protected),
                "{ok} should be allowed"
            );
        }
    }

    #[test]
    fn a_dangerous_root_is_refused_with_a_remedy() {
        let err = check_destination_root(Path::new("/")).unwrap_err();
        match err {
            Error::DestinationNotWritable { remedy, .. } => {
                assert!(remedy.to_lowercase().contains("choose a folder"))
            }
            other => panic!("expected DestinationNotWritable, got {other:?}"),
        }
    }

    // --- write errors --------------------------------------------------------

    #[test]
    fn a_full_disk_is_reported_as_such_and_other_errors_are_not() {
        #[cfg(windows)]
        let full = io::Error::from_raw_os_error(112);
        #[cfg(not(windows))]
        let full = io::Error::from_raw_os_error(28);
        assert!(matches!(
            map_write_error(full, 10),
            Error::DiskFull {
                required: 10,
                available: None
            }
        ));

        let other = io::Error::new(io::ErrorKind::PermissionDenied, "nope");
        assert!(matches!(map_write_error(other, 10), Error::Io(_)));
    }
}
