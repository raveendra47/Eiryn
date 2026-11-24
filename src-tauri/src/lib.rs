pub mod audio;
pub mod commands;
pub mod crash;
pub mod media;
pub mod session;
pub mod gpu;
pub mod hub;
pub mod lastfm;
pub mod library;
pub mod power;
pub mod queue;
pub mod queue_persistence;
pub mod radio;
pub mod remote;
pub mod search;
pub mod security;
pub mod shuffle;
pub mod state;
pub mod tray;
pub mod windows;

#[cfg(windows)]
pub use windows::{
    TaskbarManager,
    TaskbarProgressState,
    ThumbnailButton,
    JumpListEntry,
    update_jump_list,
    register_media_keys,
    enable_high_dpi,
};

#[cfg(windows)]
pub use gpu::{GpuArtworkEngine, extract_dominant_colors_gpu};


