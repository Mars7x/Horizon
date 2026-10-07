use thiserror::Error;

use crate::{persistence::PersistenceError, platform::data_paths::DataPathError};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("UI platform error: {0}")]
    Ui(#[from] slint::PlatformError),
    #[error(transparent)]
    DataPath(#[from] DataPathError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}
