//! Core of fffFinder: search engine wrapper around `fff-search`, file model,
//! folder listing, sidebar places, settings, and formatting. Shared by the
//! GUI and `nu_plugin_fff` so both see the same index roots and results.

pub mod config;
pub mod engine;
pub mod entry;
pub mod format;
pub mod kind;
pub mod listing;
pub mod places;

pub use config::{Appearance, Config, ViewMode};
pub use engine::{
    Engine, EngineOptions, GrepHit, GrepRequest, GrepResponse, RootStatus, SearchHit,
    SearchRequest, SearchResponse,
};
pub use entry::FileEntry;
pub use kind::{FileKind, KindFilter};
pub use listing::{SortKey, SortOrder};
