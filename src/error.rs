use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("UI platform error: {0}")]
    Ui(#[from] slint::PlatformError),
}
