pub mod cue;
pub mod track;
pub mod db;
pub mod playlist;
pub mod scanner;
pub mod smart_playlist;
pub mod eq;
pub mod artist_artwork;
pub mod artist_artwork_worker;
pub mod track_artwork;
pub mod palette;
pub mod windows_watcher;
pub mod loudness;
pub mod insights;
pub mod analysis;
pub mod health;
pub mod backup;

pub use windows_watcher::start_watcher as start_windows_watcher;
