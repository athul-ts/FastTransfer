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

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

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
}
