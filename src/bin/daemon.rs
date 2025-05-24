use anyhow::{Context, Result};
use std::sync::Mutex;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::signal::unix::{SignalKind, signal};
use ttd::async_socket::SocketStream;
use ttd::{
    APP_NAME, Activity, Event, IpcRequest, Status, async_socket::SocketServer, get_unix_time,
};
use ttd::{ActivityMessage, IpcResponse};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let config = Config::load().expect("failed to load config");
    let activity_log = TimeLog::init().expect("failed to init activity log");

    Daemon::new(config, activity_log).run().await?;
    Ok(())
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Config {
    activities: Vec<Activity>,
}

impl Config {
    fn load() -> Result<Self> {
        let dir = dirs::config_dir().context("no config dir")?.join(APP_NAME);
        if !dir.exists() {
            fs::create_dir_all(&dir).context("failed to create config dir")?;
        }
        let path = dir.join("config.toml");
        if path.exists() {
            let config_string =
                std::fs::read_to_string(path).context("failed to read config file")?;
            toml::from_str(&config_string).context("failed to parse config file")
        } else {
            log::warn!("no config file found, using defaults");
            Ok(Config::default())
        }
    }
}

struct TimeLog {
    file: File,
}

impl TimeLog {
    fn init() -> Result<Self> {
        let path = dirs::data_local_dir()
            .context("no data local dir")?
            .join(APP_NAME);
        if !path.exists() {
            fs::create_dir_all(path.parent().unwrap()).context("failed to create log dir")?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.join("time_log"))
            .context("failed to open time log file")?;
        let mut log = Self { file };
        log.log(Event::Power(true))?;
        Ok(log)
    }

    fn log(&mut self, event: Event) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("time went backwards")?
            .as_secs();
        writeln!(self.file, "{timestamp} {event}")?;

        Ok(())
    }
}

impl Drop for TimeLog {
    fn drop(&mut self) {
        log::info!("saving activity log");
        self.log(Event::Power(false)).unwrap();
    }
}

struct Daemon {
    config: Config,
    activity_log: TimeLog,
    started: SystemTime,
    current: Option<Activity>,
    last_active: u64,
}

impl Daemon {
    fn new(config: Config, activity_log: TimeLog) -> Self {
        Self {
            config,
            activity_log,
            started: SystemTime::now(),
            current: None,
            last_active: get_unix_time(),
        }
    }

    async fn run(self) -> Result<()> {
        let mut listener = SocketServer::create(ttd::socket_path(), true)
            .await
            .context("failed to create socket server")?;
        let mut activity_stream = SocketStream::connect(ttd::activity_daemon_socket()).await?;

        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        let daemon = Arc::new(Mutex::new(self));
        loop {
            tokio::select! {
                _ = sigterm.recv() => {
                    log::info!("received SIGTERM, shutting down");
                    break;
                }
                _ = sigint.recv() => {
                    log::info!("received SIGINT, shutting down");
                    break;
                }
                Ok(client_stream) = listener.accept_client() => {
                    tokio::spawn({
                        let daemon = daemon.clone();
                        async move {
                            if let Err(e) = Self::handle_client(client_stream, daemon).await {
                                log::error!("client handler error: {e:?}");
                            }
                        }
                    });
                }
                Ok(activity) = activity_stream.recv::<ActivityMessage>() => {
                    let mut daemon = daemon.lock().unwrap();
                    daemon.last_active = activity.last_active;
                }
            }
        }
        Ok(())
    }

    async fn handle_client(mut stream: SocketStream, daemon: Arc<Mutex<Daemon>>) -> Result<()> {
        let msg: IpcRequest = stream.recv().await?;
        let resp = {
            let mut daemon = daemon.lock().unwrap();
            daemon.handle_msg(msg)?
        };
        stream.send(resp).await?;
        Ok(())
    }

    fn handle_msg(&mut self, msg: IpcRequest) -> Result<IpcResponse> {
        Ok(match msg {
            IpcRequest::GetActivities => IpcResponse::Activities(self.config.activities.clone()),
            IpcRequest::Switch(new) => {
                if new != self.current {
                    if let Some(new_activity) = new {
                        if self.config.activities.contains(&new_activity) {
                            log::info!("switching to {}", new_activity);
                            self.activity_log
                                .log(Event::SwitchActivity(Some(new_activity.clone())))
                                .unwrap();
                            self.current = Some(new_activity);
                            self.started = SystemTime::now();
                        } else {
                            log::error!("unknown activity: {}", new_activity);
                        }
                    } else {
                        log::info!("switching to no activity");
                        self.activity_log.log(Event::SwitchActivity(None)).unwrap();
                        self.current = None;
                        self.started = SystemTime::now();
                    }
                }
                IpcResponse::Empty
            }
            IpcRequest::Status => IpcResponse::Status(Status::new(
                self.current.clone(),
                self.started.elapsed().expect("time went backwards"),
            )),
        })
    }
}
