use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[cfg(windows)]
static WINDOWS_PREFER_JUNCTION_FALLBACK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Refuse to copy when `dst` would land inside `src` (or equal `src`).
/// Otherwise the recursive copy walks into the freshly-created `dst` and
/// produces unbounded `<dst>/<dst>/<dst>/...` nesting (issue #61).
pub(crate) fn ensure_dst_not_inside_src(src: &Path, dst: &Path) -> Result<()> {
    let Ok(src_canon) = src.canonicalize() else {
        return Ok(());
    };
    let dst_canon: Option<PathBuf> = dst.canonicalize().ok().or_else(|| {
        let parent = dst.parent()?.canonicalize().ok()?;
        let name = dst.file_name()?;
        Some(parent.join(name))
    });
    if let Some(dst_canon) = dst_canon {
        if dst_canon.starts_with(&src_canon) {
            anyhow::bail!(
                "Destination {:?} is inside source {:?}; refusing to copy to avoid infinite recursion",
                dst,
                src
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Symlink,
    Junction,
    Copy,
}

impl SyncMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncMode::Symlink => "symlink",
            SyncMode::Junction => "junction",
            SyncMode::Copy => "copy",
        }
    }
}

pub fn sync_mode_for_tool(_tool_key: &str, configured_mode: Option<&str>) -> SyncMode {
    match configured_mode {
        Some("copy") => SyncMode::Copy,
        Some("junction") => SyncMode::Junction,
        Some("symlink") => SyncMode::Symlink,
        _ => SyncMode::Symlink,
    }
}

pub fn target_dir_name(central_path: &Path, skill_name: &str) -> String {
    central_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| skill_name.to_string())
}

pub fn sync_skill(source: &Path, target: &Path, mode: SyncMode) -> Result<SyncMode> {
    // Internal self-check uses no hash context, so Copy mode always
    // proceeds — the caller (e.g. `sync_desired_targets`) is the place
    // that knows about freshness and can short-circuit.
    if is_target_current(source, target, mode, None, None) {
        return Ok(mode);
    }
    #[cfg(windows)]
    if matches!(mode, SyncMode::Symlink)
        && is_target_current(source, target, SyncMode::Junction, None, None)
    {
        return Ok(SyncMode::Junction);
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent dir {:?}", parent))?;
    }

    ensure_dst_not_inside_src(source, target)?;

    // Remove existing target
    remove_target(target).ok();

    match mode {
        SyncMode::Symlink => {
            #[cfg(unix)]
            {
                create_directory_symlink(source, target).with_context(|| {
                    format!("Failed to create symlink {:?} -> {:?}", target, source)
                })?;
                Ok(SyncMode::Symlink)
            }
            #[cfg(windows)]
            {
                if windows_prefers_junction_fallback() {
                    return create_junction(source, target)
                        .map(|()| SyncMode::Junction)
                        .or_else(|junction_err| {
                            log::warn!(
                                "cached junction fallback {:?} -> {:?} failed ({junction_err}); falling back to copy",
                                target,
                                source
                            );
                            let _ = remove_target(target);
                            copy_dir_recursive(source, target)?;
                            Ok(SyncMode::Copy)
                        });
                }
                match create_directory_symlink(source, target) {
                    Ok(()) => Ok(SyncMode::Symlink),
                    Err(symlink_err) => match create_junction(source, target) {
                        Ok(()) => {
                            remember_windows_junction_fallback();
                            log::warn!(
                                "symlink_dir {:?} -> {:?} failed, using junction fallback: {symlink_err}",
                                target,
                                source
                            );
                            Ok(SyncMode::Junction)
                        }
                        Err(junction_err) => {
                            // Typical symlink causes: missing SeCreateSymbolicLinkPrivilege,
                            // Developer Mode disabled, or non-NTFS target volume.
                            log::warn!(
                                "symlink_dir {:?} -> {:?} failed ({symlink_err}); junction fallback failed ({junction_err}); falling back to copy",
                                target,
                                source
                            );
                            let _ = remove_target(target);
                            copy_dir_recursive(source, target)?;
                            Ok(SyncMode::Copy)
                        }
                    },
                }
            }
            #[cfg(all(not(unix), not(windows)))]
            {
                copy_dir_recursive(source, target)?;
                Ok(SyncMode::Copy)
            }
        }
        SyncMode::Junction => {
            #[cfg(windows)]
            {
                create_junction(source, target)?;
                Ok(SyncMode::Junction)
            }
            #[cfg(unix)]
            {
                create_directory_symlink(source, target).with_context(|| {
                    format!("Failed to create symlink {:?} -> {:?}", target, source)
                })?;
                Ok(SyncMode::Symlink)
            }
            #[cfg(all(not(unix), not(windows)))]
            {
                copy_dir_recursive(source, target)?;
                Ok(SyncMode::Copy)
            }
        }
        SyncMode::Copy => {
            copy_dir_recursive(source, target)?;
            Ok(SyncMode::Copy)
        }
    }
}

/// Decide whether the existing target is already in the desired state.
///
/// - **Symlink mode**: the target must be a symlink pointing at `source`.
/// - **Copy mode**: the target must still exist on disk **and** the
///   previously synced source hash must equal the current source hash
///   (both must be `Some`). The existence check protects against a
///   user manually deleting the synced directory between sessions —
///   without it a stale hash would cause us to skip a re-copy the
///   user needs. Callers without hash context should pass `None`,
///   which preserves the historical "always recopy" behavior. See
///   `SkillTargetRecord.source_hash` and issue #153 for context.
pub fn is_target_current(
    source: &Path,
    target: &Path,
    mode: SyncMode,
    last_synced_source_hash: Option<&str>,
    current_source_hash: Option<&str>,
) -> bool {
    match mode {
        SyncMode::Symlink => symlink_points_to(target, source),
        SyncMode::Junction => junction_points_to(target, source),
        SyncMode::Copy => match (last_synced_source_hash, current_source_hash) {
            (Some(stored), Some(current)) if stored == current => {
                std::fs::symlink_metadata(target).is_ok()
            }
            _ => false,
        },
    }
}

fn create_directory_symlink(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, target)?;
        Ok(())
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = (source, target);
        anyhow::bail!("directory symlinks are unsupported on this platform");
    }
}

fn symlink_points_to(target: &Path, source: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(target) else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        return false;
    }

    let Ok(link_target) = std::fs::read_link(target) else {
        return false;
    };
    let resolved_link_target = if link_target.is_absolute() {
        link_target
    } else {
        target
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(link_target)
    };

    if resolved_link_target == source {
        return true;
    }

    match (resolved_link_target.canonicalize(), source.canonicalize()) {
        (Ok(link), Ok(src)) => link == src,
        _ => false,
    }
}

fn junction_points_to(target: &Path, source: &Path) -> bool {
    #[cfg(windows)]
    {
        let Ok(metadata) = std::fs::symlink_metadata(target) else {
            return false;
        };
        if metadata.file_type().is_symlink() || !is_windows_reparse_point(&metadata) {
            return false;
        }
        match (target.canonicalize(), source.canonicalize()) {
            (Ok(link), Ok(src)) => link == src,
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (target, source);
        false
    }
}

#[cfg(windows)]
fn create_junction(source: &Path, target: &Path) -> Result<()> {
    match create_junction_reparse_point(source, target) {
        Ok(()) => Ok(()),
        Err(native_err) => {
            log::debug!(
                "native junction creation {:?} -> {:?} failed, trying mklink fallback: {native_err}",
                target,
                source
            );
            create_junction_with_mklink(source, target).with_context(|| {
                format!(
                    "native junction creation failed ({native_err}); mklink fallback also failed"
                )
            })
        }
    }
}

#[cfg(windows)]
fn create_junction_with_mklink(source: &Path, target: &Path) -> Result<()> {
    let source_arg = windows_cmd_path(source);
    let target_arg = windows_cmd_path(target);
    let output = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&target_arg)
        .arg(&source_arg)
        .output()
        .with_context(|| {
            format!(
                "Failed to start junction creation {:?} -> {:?}",
                target, source
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    if detail.is_empty() {
        anyhow::bail!(
            "Failed to create junction {:?} -> {:?}: {}",
            target,
            source,
            output.status
        );
    }
    anyhow::bail!(
        "Failed to create junction {:?} -> {:?}: {}",
        target,
        source,
        detail
    );
}

#[cfg(windows)]
fn windows_cmd_path(path: &Path) -> String {
    path.as_os_str().to_string_lossy().replace('/', "\\")
}

#[cfg(windows)]
fn create_junction_reparse_point(source: &Path, target: &Path) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
    use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    struct HandleGuard(windows_sys::Win32::Foundation::HANDLE);

    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let source = source
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize junction source {:?}", source))?;
    std::fs::create_dir(target)
        .with_context(|| format!("Failed to create junction directory {:?}", target))?;

    let result = (|| {
        let target_wide = wide_null(OsStr::new(&windows_cmd_path(target)));
        let handle = unsafe {
            CreateFileW(
                target_wide.as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("Failed to open junction directory {:?}", target));
        }
        let _handle = HandleGuard(handle);
        let buffer = junction_reparse_buffer(&source)?;
        let mut bytes_returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr() as *const _,
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut bytes_returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "Failed to set junction reparse point {:?} -> {:?}",
                    target, source
                )
            });
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_dir(target);
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn wide_bytes(value: &str) -> Vec<u8> {
        OsStr::new(value)
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn junction_reparse_buffer(source: &Path) -> Result<Vec<u8>> {
        let print_name = junction_print_path(source);
        let substitute_name = format!(r"\??\{print_name}");
        let substitute = wide_bytes(&substitute_name);
        let print = wide_bytes(&print_name);

        let substitute_len =
            u16::try_from(substitute.len()).context("Junction substitute path is too long")?;
        let print_offset = substitute_len
            .checked_add(2)
            .context("Junction reparse path offset overflow")?;
        let print_len = u16::try_from(print.len()).context("Junction print path is too long")?;

        let path_buffer_len = substitute.len() + 2 + print.len() + 2;
        let reparse_data_len = 8usize
            .checked_add(path_buffer_len)
            .context("Junction reparse buffer length overflow")?;
        let reparse_data_len =
            u16::try_from(reparse_data_len).context("Junction reparse buffer is too large")?;

        let mut buffer = Vec::with_capacity(8 + reparse_data_len as usize);
        buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buffer.extend_from_slice(&reparse_data_len.to_le_bytes());
        buffer.extend_from_slice(&0u16.to_le_bytes());
        buffer.extend_from_slice(&0u16.to_le_bytes());
        buffer.extend_from_slice(&substitute_len.to_le_bytes());
        buffer.extend_from_slice(&print_offset.to_le_bytes());
        buffer.extend_from_slice(&print_len.to_le_bytes());
        buffer.extend_from_slice(&substitute);
        buffer.extend_from_slice(&0u16.to_le_bytes());
        buffer.extend_from_slice(&print);
        buffer.extend_from_slice(&0u16.to_le_bytes());
        Ok(buffer)
    }

    fn junction_print_path(path: &Path) -> String {
        let normalized = windows_cmd_path(path);
        if let Some(rest) = normalized.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{rest}")
        } else if let Some(rest) = normalized.strip_prefix(r"\\?\") {
            rest.to_string()
        } else {
            normalized
        }
    }

    result.with_context(|| {
        format!(
            "Failed to create native junction {:?} -> {:?}",
            target, source
        )
    })
}

#[cfg(windows)]
fn windows_prefers_junction_fallback() -> bool {
    use std::sync::atomic::Ordering;
    WINDOWS_PREFER_JUNCTION_FALLBACK.load(Ordering::Relaxed)
}

#[cfg(windows)]
fn remember_windows_junction_fallback() {
    use std::sync::atomic::Ordering;
    WINDOWS_PREFER_JUNCTION_FALLBACK.store(true, Ordering::Relaxed);
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(windows)]
fn is_windows_directory_like(metadata: &std::fs::Metadata, target: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    target.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
}

pub fn remove_target(target: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    #[cfg(windows)]
    if is_windows_reparse_point(&metadata) {
        if is_windows_directory_like(&metadata, target) {
            std::fs::remove_dir(target)?;
        } else {
            std::fs::remove_file(target)?;
        }
        return Ok(());
    }

    if metadata.file_type().is_symlink() {
        #[cfg(windows)]
        {
            if target.is_dir() {
                std::fs::remove_dir(target)?;
            } else {
                std::fs::remove_file(target)?;
            }
        }
        #[cfg(not(windows))]
        {
            std::fs::remove_file(target)?;
        }
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(target)?;
    } else {
        std::fs::remove_file(target)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ft.is_dir() {
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── sync_mode_for_tool ──

    #[test]
    fn sync_mode_defaults_to_symlink() {
        assert!(matches!(
            sync_mode_for_tool("claude-code", None),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_cursor_defaults_to_symlink() {
        assert!(matches!(
            sync_mode_for_tool("cursor", None),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_explicit_copy_overrides_default() {
        assert!(matches!(
            sync_mode_for_tool("claude-code", Some("copy")),
            SyncMode::Copy
        ));
    }

    #[test]
    fn sync_mode_explicit_symlink_overrides_cursor_default() {
        assert!(matches!(
            sync_mode_for_tool("cursor", Some("symlink")),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_explicit_junction_is_supported() {
        assert!(matches!(
            sync_mode_for_tool("cursor", Some("junction")),
            SyncMode::Junction
        ));
    }

    #[test]
    fn sync_mode_unknown_config_falls_back_to_tool_default() {
        assert!(matches!(
            sync_mode_for_tool("cursor", Some("invalid")),
            SyncMode::Symlink
        ));
        assert!(matches!(
            sync_mode_for_tool("claude-code", Some("invalid")),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_as_str() {
        assert_eq!(SyncMode::Symlink.as_str(), "symlink");
        assert_eq!(SyncMode::Junction.as_str(), "junction");
        assert_eq!(SyncMode::Copy.as_str(), "copy");
    }

    #[test]
    fn target_dir_name_uses_central_directory_name() {
        let central_path = Path::new("/central/skill123-2");

        assert_eq!(target_dir_name(central_path, "skill123"), "skill123-2");
    }

    #[test]
    fn target_dir_name_falls_back_to_skill_name() {
        assert_eq!(target_dir_name(Path::new(""), "skill123"), "skill123");
    }

    // ── sync_skill (filesystem) ──

    #[test]
    fn sync_skill_copy_creates_directory_with_files() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Copy).unwrap();
        assert!(matches!(mode, SyncMode::Copy));
        assert!(tgt.join("SKILL.md").exists());
        assert_eq!(fs::read_to_string(tgt.join("SKILL.md")).unwrap(), "# hello");
    }

    #[cfg(unix)]
    #[test]
    fn sync_skill_symlink_creates_symlink() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();
        assert!(matches!(mode, SyncMode::Symlink));
        assert!(tgt.is_symlink());
    }

    #[cfg(windows)]
    #[test]
    fn sync_skill_symlink_uses_directory_link_on_windows() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();
        assert_ne!(mode.as_str(), "copy");
        assert_eq!(tgt.canonicalize().unwrap(), src.canonicalize().unwrap());
        assert_eq!(fs::read_to_string(tgt.join("SKILL.md")).unwrap(), "# hello");
    }

    #[test]
    fn sync_skill_replaces_existing_target() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("new.md"), "new").unwrap();

        // Pre-existing target directory
        fs::create_dir_all(&tgt).unwrap();
        fs::write(tgt.join("old.md"), "old").unwrap();

        sync_skill(&src, &tgt, SyncMode::Copy).unwrap();
        assert!(tgt.join("new.md").exists());
        assert!(!tgt.join("old.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn sync_skill_symlink_skips_existing_correct_link() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&src, &tgt).unwrap();

        let before = fs::symlink_metadata(&tgt).unwrap().modified().unwrap();
        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();

        assert!(matches!(mode, SyncMode::Symlink));
        assert_eq!(fs::read_link(&tgt).unwrap(), src);
        assert_eq!(
            fs::symlink_metadata(&tgt).unwrap().modified().unwrap(),
            before
        );
    }

    // ── copy_dir_recursive ──

    #[test]
    fn copy_dir_recursive_skips_dot_git() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join(".git")).unwrap();
        fs::write(src.join(".git/config"), "git config").unwrap();
        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("subdir/file.md"), "content").unwrap();
        fs::write(src.join("root.md"), "root").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).unwrap();

        assert!(!dst.join(".git").exists());
        assert!(dst.join("subdir/file.md").exists());
        assert!(dst.join("root.md").exists());
    }

    // ── ensure_dst_not_inside_src ──

    #[test]
    fn ensure_dst_not_inside_src_rejects_subdirectory() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        fs::create_dir_all(&src).unwrap();
        let dst = src.join("skills");

        let err = ensure_dst_not_inside_src(&src, &dst).unwrap_err();
        assert!(err.to_string().contains("infinite recursion"), "{err}");
    }

    #[test]
    fn ensure_dst_not_inside_src_rejects_same_path() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        fs::create_dir_all(&src).unwrap();

        let err = ensure_dst_not_inside_src(&src, &src).unwrap_err();
        assert!(err.to_string().contains("infinite recursion"), "{err}");
    }

    #[test]
    fn ensure_dst_not_inside_src_allows_disjoint_paths() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        let dst = tmp.path().join("other").join("skills");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dst.parent().unwrap()).unwrap();

        ensure_dst_not_inside_src(&src, &dst).unwrap();
    }

    #[test]
    fn ensure_dst_not_inside_src_allows_sibling_dst() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        let dst = tmp.path().join("skills-disabled");
        fs::create_dir_all(&src).unwrap();

        ensure_dst_not_inside_src(&src, &dst).unwrap();
    }

    #[test]
    fn sync_skill_refuses_target_inside_source() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        let tgt = src.join("skills");

        let err = sync_skill(&src, &tgt, SyncMode::Copy).unwrap_err();
        assert!(err.to_string().contains("infinite recursion"), "{err}");
        // Source must be untouched after the rejection.
        assert!(src.join("SKILL.md").exists());
    }

    // ── remove_target ──

    #[test]
    fn remove_target_removes_directory() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("to_remove");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), "data").unwrap();

        remove_target(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn remove_target_removes_file() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("file.txt");
        fs::write(&file, "data").unwrap();

        remove_target(&file).unwrap();
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn remove_target_removes_symlink() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        remove_target(&link).unwrap();
        assert!(!link.exists());
        assert!(real.exists()); // original untouched
    }

    #[cfg(windows)]
    #[test]
    fn remove_target_removes_directory_symlink() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        let link = tmp.path().join("link");
        if let Err(err) = std::os::windows::fs::symlink_dir(&real, &link) {
            if err.raw_os_error() == Some(1314) {
                return;
            }
            panic!("failed to create directory symlink: {err}");
        }

        remove_target(&link).unwrap();
        assert!(!link.exists());
        assert!(real.exists());
        assert!(real.join("SKILL.md").exists());
    }

    #[cfg(windows)]
    #[test]
    fn remove_target_removes_junction_without_deleting_source() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        let link = tmp.path().join("link");
        create_junction(&real, &link).unwrap();

        remove_target(&link).unwrap();
        assert!(!link.exists());
        assert!(real.exists());
        assert_eq!(
            fs::read_to_string(real.join("SKILL.md")).unwrap(),
            "# hello"
        );
    }

    #[cfg(windows)]
    #[test]
    fn create_junction_accepts_paths_with_forward_slashes() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("sourceroot");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        let link = tmp.path().join("targetroot").join("skills").join("link");
        fs::create_dir_all(link.parent().unwrap()).unwrap();

        let real_with_forward_slashes = PathBuf::from(real.to_string_lossy().replace('\\', "/"));
        let link_with_forward_slashes = PathBuf::from(link.to_string_lossy().replace('\\', "/"));

        create_junction(&real_with_forward_slashes, &link_with_forward_slashes).unwrap();

        assert_eq!(link.canonicalize().unwrap(), real.canonicalize().unwrap());
        assert_eq!(
            fs::read_to_string(link.join("SKILL.md")).unwrap(),
            "# hello"
        );
    }

    #[test]
    fn remove_target_nonexistent_is_ok() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("does_not_exist");
        assert!(remove_target(&path).is_ok());
    }

    // ── is_target_current copy-mode freshness (issue #153) ──

    #[test]
    fn is_target_current_copy_skips_when_hashes_match_and_target_exists() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&tgt).unwrap();
        assert!(is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-abc"),
            Some("hash-abc"),
        ));
    }

    #[test]
    fn is_target_current_copy_resyncs_when_target_missing_even_if_hashes_match() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target-that-was-deleted");
        // User deleted the synced directory manually; we must re-copy.
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-abc"),
            Some("hash-abc"),
        ));
    }

    #[test]
    fn is_target_current_copy_resyncs_when_hashes_differ() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&tgt).unwrap();
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-old"),
            Some("hash-new"),
        ));
    }

    #[test]
    fn is_target_current_copy_resyncs_when_either_hash_missing() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&tgt).unwrap();
        // No previously recorded hash → must resync (e.g. row predates v6).
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            None,
            Some("hash-abc"),
        ));
        // Source has no current hash → must resync (defensive).
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-abc"),
            None,
        ));
        // Both missing → must resync.
        assert!(!is_target_current(&src, &tgt, SyncMode::Copy, None, None));
    }
}
