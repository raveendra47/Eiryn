//! Windows-specific file watcher using ReadDirectoryChangesW API
//! 
//! This provides instant file change detection without polling,
//! replacing the cross-platform notify crate for better performance.

#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use sqlx::SqlitePool;
#[cfg(windows)]
use tokio::sync::mpsc;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
    FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME, FILE_NOTIFY_INFORMATION,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
};
#[cfg(windows)]
use windows::Win32::System::Threading::Sleep;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::ReadDirectoryChangesW;

/// File change event types
#[derive(Debug, Clone)]
pub enum FileChangeEvent {
    Added(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    Renamed(PathBuf, PathBuf), // (old_path, new_path)
}

/// Windows directory watcher using ReadDirectoryChangesW
pub struct WindowsWatcher {
    handle: Option<HANDLE>,
    watch_path: PathBuf,
    event_tx: mpsc::Sender<FileChangeEvent>,
}

// SAFETY: Windows HANDLEs are kernel object handles and are thread-safe.
// The watcher only calls ReadDirectoryChangesW and CloseHandle, both of which are safe across threads.
unsafe impl Send for WindowsWatcher {}
unsafe impl Sync for WindowsWatcher {}

impl WindowsWatcher {
    /// Create a new watcher for the specified directory
    pub fn new<P: AsRef<Path>>(path: P, event_tx: mpsc::Sender<FileChangeEvent>) -> Result<Self, String> {
        #[cfg(not(windows))]
        {
            return Err("WindowsWatcher is only available on Windows".to_string());
        }
        
        #[cfg(windows)]
        {
            let path = path.as_ref().to_path_buf();
            
            // Open directory handle
            let handle = unsafe {
                use std::ffi::OsStr;
                use std::os::windows::ffi::OsStrExt;
                
                let wide_path: Vec<u16> = OsStr::new(&path)
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                
                CreateFileW(
                    windows::core::PCWSTR(wide_path.as_ptr()),
                    GENERIC_READ.0, // ReadDirectoryChangesW requires GENERIC_READ on the directory handle
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None, // no security attributes
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS, // Required for directories
                    HANDLE::default(),
                )
            };
            
            let handle = handle.map_err(|e| format!("Failed to open directory: {:?} (error: {:?})", path, e))?;
            if handle == INVALID_HANDLE_VALUE {
                return Err(format!("Failed to open directory: {:?}", path));
            }
            
            Ok(Self {
                handle: Some(handle),
                watch_path: path,
                event_tx,
            })
        }
    }
    
    /// Blocking read loop. The watcher owns the directory handle for the
    /// entire loop, so the handle can never be closed while
    /// ReadDirectoryChangesW is in flight. Runs on a dedicated std::thread
    /// (ReadDirectoryChangesW blocks, so it must not sit on a Tokio worker).
    pub fn read_loop(&self) {
        #[cfg(not(windows))]
        {
            return;
        }

        #[cfg(windows)]
        {
            let Some(handle) = self.handle else {
                log::error!("Watcher handle not initialized, read loop exiting");
                return;
            };
            // Store as isize to make it Send (HANDLE wraps *mut c_void which is !Send)
            let handle_ptr = handle.0 as isize;
            let watch_path = self.watch_path.clone();
            let event_tx = self.event_tx.clone();

            const BUFFER_SIZE: usize = 4096;
            let mut buffer = vec![0u8; BUFFER_SIZE];
            // Windows reports a rename as two records — RENAMED_OLD_NAME then
            // RENAMED_NEW_NAME in the same buffer. Pair them up.
            let mut pending_old: Option<PathBuf> = None;

            loop {
                // Wait for directory changes
                let mut bytes_returned = 0u32;
                let result = unsafe {
                    ReadDirectoryChangesW(
                        HANDLE(handle_ptr as *mut _),
                        buffer.as_mut_ptr() as *mut core::ffi::c_void,
                        BUFFER_SIZE as u32,
                        false, // non-recursive
                        FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE,
                        Some(&mut bytes_returned as *mut u32),
                        None, // no overlapped I/O
                        None, // no completion routine
                    )
                };

                if result.is_err() || bytes_returned == 0 {
                    log::error!("ReadDirectoryChangesW failed (result: {:?})", result);
                    unsafe { Sleep(1000); } // Wait before retrying
                    continue;
                }

                // Parse FILE_NOTIFY_INFORMATION records. The fixed header
                // (NextEntryOffset + Action + FileNameLength) is 12 bytes;
                // the file name starts immediately after it.
                let mut offset = 0usize;
                while offset < bytes_returned as usize {
                    let notify = unsafe {
                        &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION)
                    };

                    let action = notify.Action;
                    let filename_len = notify.FileNameLength as usize;
                    let filename = unsafe {
                        String::from_utf16_lossy(std::slice::from_raw_parts(
                            notify.FileName.as_ptr(),
                            filename_len / 2,
                        ))
                    };

                    let full_path = watch_path.join(&filename);

                    // Send event based on action
                    match action {
                        FILE_ACTION_ADDED => {
                            let _ = event_tx.blocking_send(FileChangeEvent::Added(full_path));
                        }
                        FILE_ACTION_MODIFIED => {
                            let _ = event_tx.blocking_send(FileChangeEvent::Modified(full_path));
                        }
                        FILE_ACTION_REMOVED => {
                            let _ = event_tx.blocking_send(FileChangeEvent::Removed(full_path));
                        }
                        FILE_ACTION_RENAMED_OLD_NAME => {
                            pending_old = Some(full_path);
                        }
                        FILE_ACTION_RENAMED_NEW_NAME => {
                            match pending_old.take() {
                                Some(old) => {
                                    let _ = event_tx.blocking_send(FileChangeEvent::Renamed(old, full_path));
                                }
                                None => {
                                    let _ = event_tx.blocking_send(FileChangeEvent::Added(full_path));
                                }
                            }
                        }
                        _ => {}
                    }

                    // Move to next record
                    offset = if notify.NextEntryOffset == 0 {
                        break;
                    } else {
                        offset + notify.NextEntryOffset as usize
                    };
                }
            }
        }
    }
}

impl Drop for WindowsWatcher {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if let Some(handle) = self.handle.take() {
                unsafe {
                    let _ = CloseHandle(handle);
                }
            }
        }
    }
}

/// Start a Windows-specific file watcher that triggers library rescans
#[cfg(windows)]
pub fn start_watcher(pool: SqlitePool, folder: PathBuf) -> Result<(), String> {
    use crate::library::scanner::scan_folder;
    use crate::library::track_artwork::TrackArtworkCache;
    
    let (tx, mut rx) = mpsc::channel::<FileChangeEvent>(100);
    
    // Create the watcher and move it into a dedicated std::thread. The thread
    // owns the watcher (and thus the directory handle) for the entire read
    // loop, so the handle is never closed while ReadDirectoryChangesW is in
    // flight, and the blocking API never occupies a Tokio worker.
    let watcher = WindowsWatcher::new(&folder, tx)?;
    std::thread::spawn(move || {
        watcher.read_loop();
    });
    
    // Debounce loop: coalesce raw events into a periodic rescan.
    // tauri::async_runtime::spawn (not tokio::spawn): start_watcher is called
    // from the sync setup hook on the main thread, which has no tokio context.
    tauri::async_runtime::spawn(async move {
        // Simple debouncing: collect events within a time window
        let mut pending_changes = std::collections::HashSet::new();
        let debounce_duration = tokio::time::Duration::from_secs(2);
        
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    match event {
                        FileChangeEvent::Added(path) | FileChangeEvent::Modified(path) => {
                            if !crate::library::scanner::downloading_now(&path, &folder) {
                                pending_changes.insert(path);
                            }
                        }
                        FileChangeEvent::Removed(path) => {
                            pending_changes.insert(path);
                        }
                        FileChangeEvent::Renamed(old, new) => {
                            pending_changes.remove(&old);
                            if !crate::library::scanner::downloading_now(&new, &folder) {
                                pending_changes.insert(new);
                            }
                        }
                    }
                }
                _ = tokio::time::sleep(debounce_duration) => {
                    if !pending_changes.is_empty() {
                        log::info!("Detected {} file changes, rescanning...", pending_changes.len());
                        
                        // Rescan the folder
                        let artwork_cache = TrackArtworkCache::new(folder.join(".artwork_cache"));
                        match scan_folder(&pool, &folder, &artwork_cache).await {
                            Ok(result) => {
                                log::info!("Rescan complete: added={}, updated={}, removed={}", 
                                    result.added, result.updated, result.removed);
                            }
                            Err(e) => {
                                log::error!("Rescan failed: {}", e);
                            }
                        }
                        
                        pending_changes.clear();
                    }
                }
            }
        }
    });
    
    Ok(())
}

#[cfg(not(windows))]
pub fn start_watcher(_pool: SqlitePool, _folder: PathBuf) -> Result<(), String> {
    Err("Windows file watcher is only available on Windows".to_string())
}
