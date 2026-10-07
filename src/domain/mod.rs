//! Pure application-domain types.
//!
//! This module must not depend on Slint, SQLite, SDL, Flatpak, portals,
//! launcher-specific formats, or other infrastructure concerns.

pub mod library;

pub use library::{
    DomainValidationError, ExternalGameId, Game, GameId, GameTitle, LibraryGame, SourceGameRef,
    SourceId,
};
