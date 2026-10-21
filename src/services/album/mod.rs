//! The Album: finding captures, and decoding them off the UI thread.
//!
//! - `catalog`: Horizon's capture folder and optional capture-source boundary.
//! - `media`: image decoding and the thumbnail cache.
//! - `loader`: worker threads that decode what the screen currently needs.
mod animation;
pub mod catalog;
pub mod loader;
pub mod media;

pub use animation::{AnimationPlayback, play_animation};
pub use catalog::{HorizonAlbum, part_path, scan_all};
pub use loader::{MediaLoader, MediaResult};
pub use media::{ALBUM_CACHE_VERSION, MediaError, THUMBNAIL_SIZE, ThumbnailCache};
