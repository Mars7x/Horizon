use thiserror::Error;

use crate::{
    persistence::{PersistenceError, settings::SettingsStoreError},
    platform::data_paths::DataPathError,
    services::import::SourceImportError,
    sources::SourceRegistryBuildError,
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
    Settings(#[from] SettingsStoreError),
    #[error(transparent)]
    SourceRegistryBuild(#[from] SourceRegistryBuildError),
    #[error(transparent)]
    SourceImport(#[from] SourceImportError<PersistenceError>),
}
