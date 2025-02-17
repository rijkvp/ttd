use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::iterator::Signals;
use std::sync::Mutex;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    sync::{mpsc, Arc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use ttd::{socket::SocketServer, Activity, Message, APP_NAME};

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

struct ActivityLog {
    file: File,
}

impl ActivityLog {
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
            .open(&path.join("activity_log"))
            .context("failed to open activity log file")?;
        let mut log = Self { file };
        log.log("start")?;
        Ok(log)
    }

    fn log(&mut self, key: &str) -> Result<()> {
        debug_assert!(!key.contains(' '), "key must not contain spaces");
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("time went backwards")?
            .as_secs();
        writeln!(self.file, "{} {}", timestamp, key)?;

        Ok(())
    }
}

impl Drop for ActivityLog {
    fn drop(&mut self) {
        log::info!("saving activity log");
        self.log("stop").unwrap();
    }
}

struct Daemon {
    config: Config,
    activity_log: ActivityLog,
    current: Option<Activity>,
}

impl Daemon {
    fn new(config: Config, activity_log: ActivityLog) -> Self {
        Self {
            config,
            activity_log,
            current: None,
        }
    }

    fn run(self) -> Result<()> {
        let mut server = SocketServer::create(ttd::socket_path(), false)?;

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
                    let msg: Message = rmp_serde::from_read(msg).ok()?;
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

    fn handle_msg(&mut self, msg: Message) -> Option<Vec<u8>> {
        match msg {
            Message::GetList => Some(rmp_serde::to_vec(&self.config.activities).unwrap()),
            Message::Switch(key) => {
                if Some(&key) != self.current.as_ref().map(|a| &a.id) {
                    if let Some(activity) = self.config.activities.iter().find(|a| a.id == key) {
                        log::info!("switching to {:?}", activity);
                        self.activity_log.log(&key).unwrap();
                        self.current = Some(activity.clone());
                    } else {
                        log::warn!("unknown activity: {}", key);
                    }
                }
                None
            }
        }
    }
}

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let config = Config::load().expect("failed to load config");
    let activity_log = ActivityLog::init().expect("failed to init activity log");

    Daemon::new(config, activity_log).run()?;
    Ok(())
}
