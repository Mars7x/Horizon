use thiserror::Error;

use crate::{
    persistence::PersistenceError,
    platform::data_paths::DataPathError,
    services::import::SourceImportError,
    sources::{SourceRegistryError, steam::SteamSourceInitError},
};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("UI platform error: {0}")]
    Ui(#[from] slint::PlatformError),
    #[error(transparent)]
    DataPath(#[from] DataPathError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    SteamSource(#[from] SteamSourceInitError),
    #[error(transparent)]
    SourceRegistry(#[from] SourceRegistryError),
    #[error(transparent)]
    SourceImport(#[from] SourceImportError<PersistenceError>),
}
