//! The Album: finding captures, and decoding them off the UI thread.
//!
//! - `catalog`: Horizon's capture folder and the capture sources (Steam).
//! - `export`: saving a copy to the user's Pictures folder.
//! - `media`: photo decoding, the thumbnail cache, the video backend contract.
//! - `loader`: worker threads that decode what the screen currently needs.
pub mod catalog;
pub mod export;
pub mod loader;
pub mod media;

pub use catalog::{HorizonAlbum, part_path, scan_all};
pub use loader::{MediaLoader, MediaResult};
pub use media::{
    ALBUM_CACHE_VERSION, MediaError, NoVideo, THUMBNAIL_SIZE, ThumbnailCache, VideoBackend,
    VideoFrame, VideoPlayback, VideoSink,
};
