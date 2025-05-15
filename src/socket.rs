use anyhow::{Context, Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::Shutdown,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
};

pub struct SocketServer {
    listener: UnixListener,
    path: PathBuf,
}

const EOT: u8 = 4; // End of Transmission (EOT) character

impl SocketServer {
    pub fn create(path: PathBuf, set_permissions: bool) -> Result<Self> {
        if let Some(run_dir) = path.parent() {
            fs::create_dir_all(run_dir)
                .with_context(|| format!("failed to create runtime directory '{run_dir:?}'"))?;
        }
        if path.exists() {
            log::warn!("removing exsisting socket '{}'", path.display());
            fs::remove_file(&path).with_context(|| "failed to remove existing socket")?;
        }
        let listener = UnixListener::bind(&path)
            .with_context(|| format!("failed to bind socket at '{path:?}'"))?;
        if set_permissions {
            // set Unix permissions such that all users can write to the socket
            fs::set_permissions(&path, fs::Permissions::from_mode(0o722)).unwrap();
        }
        log::info!("created at socket at '{}'", path.display());
        Ok(Self { listener, path })
    }

    pub fn send(&mut self, msg: &[u8]) -> Result<()> {
        let mut stream = self.listener.accept()?.0;
        let encoded = STANDARD_NO_PAD.encode(msg);
        stream.write_all(&[encoded.as_bytes(), &[EOT]].concat())?;
        stream.flush()?;
        Ok(())
    }

    pub fn handle<F>(&mut self, f: F) -> Result<()>
    where
        F: Fn(&[u8]) -> Option<Vec<u8>>,
    {
        if let Ok((mut stream, _)) = self.listener.accept() {
            let reader = std::io::BufReader::new(stream.try_clone()?);
            for msg in reader.split(EOT) {
                let msg = msg?;
                let decoded = STANDARD_NO_PAD.decode(&msg)?;
                if let Some(resp) = f(&decoded) {
                    let encoded = STANDARD_NO_PAD.encode(&resp);
                    stream.write_all(&[encoded.as_bytes(), &[EOT]].concat())?;
                } else {
                    stream.write_all(&[EOT])?;
                }
                stream.flush()?;
            }
        }
        Ok(())
    }
}

impl Drop for SocketServer {
    fn drop(&mut self) {
        fs::remove_file(&self.path).unwrap();
    }
}

pub struct SocketClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl SocketClient {
    pub fn connect(path: PathBuf) -> Result<Self> {
        let stream = UnixStream::connect(&path)
            .with_context(|| format!("failed to connect to socket '{path:?}'"))?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { stream, reader })
    }

    pub fn try_send(&mut self, msg: &[u8]) -> Result<Option<Vec<u8>>> {
        let encoded = STANDARD_NO_PAD.encode(msg);
        self.stream
            .write_all(&[encoded.as_bytes(), &[EOT]].concat())?;
        self.stream.flush()?;
        let mut response = Vec::new();
        self.reader.read_until(EOT, &mut response)?;
        response.pop();
        if response.is_empty() {
            return Ok(None);
        }
        let decoded = STANDARD_NO_PAD.decode(&response)?;
        Ok(Some(decoded))
    }

    pub fn send(&mut self, msg: &[u8]) -> Result<Vec<u8>> {
        self.try_send(msg)?
            .ok_or_else(|| anyhow!("empty response: server error"))
    }

    /// Receive a message from the server and handle it with the provided closure.
    /// This blocks until a message is received.
    pub fn receive<F>(&mut self, mut handle: F) -> Result<()>
    where
        F: FnMut(&[u8]),
    {
        let mut message = Vec::new();
        let bytes_read = self.reader.read_until(EOT, &mut message)?;
        if bytes_read == 0 {
            // connection closed
            log::info!("socket connection closed!!");
            return Ok(());
        }
        log::info!("received message!");
        message.pop(); // remove EOT
        if message.is_empty() {
            // skip empty message
            log::info!("skipping empty message");
            return Ok(());
        }
        let decoded = STANDARD_NO_PAD.decode(&message)?;
        handle(&decoded);
        Ok(())
    }
}

impl Drop for SocketClient {
    fn drop(&mut self) {
        self.stream.shutdown(Shutdown::Write).unwrap();
    }
}
