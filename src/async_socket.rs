use anyhow::{Context, Result};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::net::{UnixListener, UnixStream};

pub async fn create_socket_listener(path: PathBuf, set_permissions: bool) -> Result<UnixListener> {
    if let Some(run_dir) = path.parent() {
        fs::create_dir_all(run_dir)
            .with_context(|| format!("failed to create runtime directory '{run_dir:?}'"))?;
    }
    if path.exists() {
        log::warn!("removing exsisting socket '{}'", path.display());
        fs::remove_file(&path).with_context(|| "failed to remove existing socket")?;
    }
    let listener = tokio::net::UnixListener::bind(&path)
        .with_context(|| format!("failed to bind socket at '{path:?}'"))?;
    if set_permissions {
        // set Unix permissions such that all users can write to the socket
        fs::set_permissions(&path, fs::Permissions::from_mode(0o722)).unwrap();
    }
    log::info!("created at socket at '{}'", path.display());
    Ok(listener)
}

pub async fn create_socket_stream(path: PathBuf) -> Result<UnixStream> {
    let stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("failed to connect to socket at '{path:?}'"))?;
    Ok(stream)
}
