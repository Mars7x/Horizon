use ashpd::{Uri, desktop::open_uri::OpenFileRequest};
use thiserror::Error;

use crate::{
    services::launch::{LaunchExecutionError, LaunchExecutor},
    sources::SourceLaunchTarget,
};

#[derive(Debug, Error)]
enum PortalLaunchError {
    #[error("launch URI is invalid: {0}")]
    InvalidUri(String),
    #[error("could not create portal runtime: {0}")]
    Runtime(#[from] std::io::Error),
    #[error("OpenURI portal request failed: {0}")]
    Portal(#[from] ashpd::Error),
}

/// Dispatches launch URIs through XDG Desktop Portal rather than assuming a
/// host launcher binary is visible inside Horizon's Flatpak sandbox.
pub struct PortalLaunchExecutor;

impl LaunchExecutor for PortalLaunchExecutor {
    fn execute(&self, target: &SourceLaunchTarget) -> Result<(), LaunchExecutionError> {
        match target {
            SourceLaunchTarget::Uri(value) => open_uri(value).map_err(LaunchExecutionError::new),
        }
    }
}

fn open_uri(value: &str) -> Result<(), PortalLaunchError> {
    let uri = Uri::parse(value).map_err(|error| PortalLaunchError::InvalidUri(error.to_string()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        OpenFileRequest::default().send_uri(&uri).await?.response()?;
        Ok(())
    })
}
