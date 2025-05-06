use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::iterator::Signals;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    sync::{Arc, mpsc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use ttd::socket::SocketClient;
use ttd::{APP_NAME, Activity, IpcMessage, socket::SocketServer};
use ttd::{Event, Status, get_unix_time};

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let config = Config::load().expect("failed to load config");
    let activity_log = TimeLog::init().expect("failed to init activity log");

    Daemon::new(config, activity_log).run()?;
    Ok(())
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
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
    since: SystemTime,
    current: Option<Activity>,
}

impl Daemon {
    fn new(config: Config, activity_log: TimeLog) -> Self {
        Self {
            config,
            activity_log,
            since: SystemTime::now(),
            current: None,
        }
    }

    fn run(self) -> Result<()> {
        let mut server = SocketServer::create(ttd::socket_path(), false)?;

        let mut activity_client = SocketClient::connect(ttd::activity_daemon_socket())
            .context("failed to connect to activity client")?;

        let last_active = Arc::new(AtomicU64::new(get_unix_time()));
        thread::spawn({
            let last_active = last_active.clone();
            move || loop {
                if let Err(e) = activity_client.receive(|_| {
                    log::info!("active at {last_active:?}");
                    last_active.store(get_unix_time(), Ordering::Relaxed);
                }) {
                    log::error!("activity client error: {}", e);
                }
            }
        });
        thread::spawn(move || {
            loop {
                let elapsed = get_unix_time() - last_active.load(Ordering::Relaxed);
                if elapsed > 5 {
                    log::info!("no activity for 5 seconds");
                }
                thread::sleep(Duration::from_secs(1));
            }
        });

        let daemon = Arc::new(Mutex::new(Some(self)));

        let (tx, rx) = mpsc::channel();
        let mut signals = Signals::new(TERM_SIGNALS)?;
        let handle = signals.handle();
        thread::spawn(move || {
            for signal in signals.forever() {
                log::info!("received signal {:?}", signal);
                tx.send(()).unwrap();
            }
        });

        thread::spawn({
            let daemon = daemon.clone();
            move || loop {
                if let Err(e) = server.handle(|msg| {
                    let msg: IpcMessage = rmp_serde::from_read(msg).ok()?;
                    daemon.lock().unwrap().as_mut().unwrap().handle_msg(msg)
                }) {
                    log::error!("server error: {}", e);
                }
            }
        });
        let _ = rx.recv(); // block until signal
        log::info!("shutting down");
        handle.close();
        let _ = daemon.lock().unwrap().take(); // ensure daemon is dropped
        Ok(())
    }

    fn handle_msg(&mut self, msg: IpcMessage) -> Option<Vec<u8>> {
        match msg {
            IpcMessage::List => Some(rmp_serde::to_vec(&self.config.activities).unwrap()),
            IpcMessage::Switch(new) => {
                if new != self.current {
                    if let Some(new_activity) = new {
                        if self.config.activities.iter().any(|a| *a == new_activity) {
                            log::info!("switching to {}", new_activity);
                            self.activity_log
                                .log(Event::SwitchActivity(Some(new_activity.clone())))
                                .unwrap();
                            self.current = Some(new_activity);
                            self.since = SystemTime::now();
                        } else {
                            log::error!("unknown activity: {}", new_activity);
                        }
                    } else {
                        log::info!("switching to no activity");
                        self.activity_log.log(Event::SwitchActivity(None)).unwrap();
                        self.current = None;
                        self.since = SystemTime::now();
                    }
                }
                None
            }
            IpcMessage::Status => Some(
                rmp_serde::to_vec(&Status::new(
                    self.current.clone(),
                    self.since.elapsed().expect("time went backwards"),
                ))
                .unwrap(),
            ),
        }
    }
}
