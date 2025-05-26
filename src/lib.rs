pub mod async_socket;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    str::FromStr,
    time::{Duration, SystemTime},
};

pub const APP_NAME: &str = env!("CARGO_PKG_NAME");
pub const ACTIVITY_DAEMON_NAME: &str = "actived";

pub fn activity_daemon_socket() -> PathBuf {
    PathBuf::from("/run")
        .join(APP_NAME)
        .join(ACTIVITY_DAEMON_NAME)
        .with_extension("sock")
}

pub fn get_unix_time() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Debug, Clone)]
pub struct TimedEvent {
    pub timestamp: i64,
    pub event: Event,
}

impl Display for TimedEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.timestamp, self.event)
    }
}

impl FromStr for TimedEvent {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let (timestamp, event) = s
            .split_once(' ')
            .ok_or_else(|| anyhow!("invalid timed event format"))?;
        let timestamp = timestamp
            .parse::<i64>()
            .context("failed to parse timestamp")?;
        let event = event.parse()?;
        Ok(Self { timestamp, event })
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Power(bool),
    SwitchActivity(Option<Activity>),
}

impl Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Power(on) => {
                write!(f, "P ")?;
                if *on {
                    write!(f, "on")
                } else {
                    write!(f, "off")
                }
            }
            Self::SwitchActivity(activity) => {
                write!(f, "A ")?;
                if let Some(activity) = activity {
                    write!(f, "{activity}")
                } else {
                    write!(f, "-")
                }
            }
        }
    }
}

impl FromStr for Event {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parts = s.splitn(2, ' ');
        let kind = parts.next().ok_or_else(|| anyhow!("empty event"))?;
        let rest = parts.next().ok_or_else(|| anyhow!("missing event data"))?;
        match kind {
            "P" => {
                let on = match rest {
                    "on" => true,
                    "off" => false,
                    _ => bail!("invalid power state: '{}'", rest),
                };
                Ok(Self::Power(on))
            }
            "A" => {
                let activity = if rest == "-" {
                    None
                } else {
                    Some(Activity::new(rest.to_string())?)
                };
                Ok(Self::SwitchActivity(activity))
            }
            _ => bail!("invalid event kind: '{}'", kind),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Activity {
    key: String,
}

impl Activity {
    pub fn new(key: String) -> Result<Self> {
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ':')
        {
            bail!("invalid activity: '{}'", key);
        }
        Ok(Self { key })
    }
}

impl Display for Activity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityMessage {
    pub last_active: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    Status,
    Switch(Option<Activity>),
    GetActivities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Empty,
    Status(Status),
    Activities(Vec<Activity>),
}

pub fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .expect("No runtime directory found!")
        .join(APP_NAME)
        .join(APP_NAME)
        .with_extension("sock")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    activity: Option<Activity>,
    duration: Duration,
}

impl Status {
    pub fn new(activity: Option<Activity>, duration: Duration) -> Self {
        Self { activity, duration }
    }
}

fn format_duration(f: &mut fmt::Formatter, duration: Duration) -> fmt::Result {
    let mut secs = duration.as_secs();
    let days = secs / 86_400;
    secs %= 86_400;
    let hours = secs / 3_600;
    secs %= 3_600;
    let minutes = secs / 60;
    secs %= 60;
    if days > 0 {
        write!(f, "{}d ", days)?;
    }
    if hours > 0 {
        write!(f, "{:02}:", hours)?;
    }
    write!(f, "{:02}:{:02}", minutes, secs)
}

impl Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(activity) = &self.activity {
            write!(f, "{activity}")?;
        } else {
            write!(f, "(no activity)")?;
        }
        write!(f, " ")?;
        format_duration(f, self.duration)?;
        Ok(())
    }
}

fn activity_log_path() -> Result<PathBuf> {
    let path = dirs::data_local_dir()
        .context("no data local dir")?
        .join(APP_NAME);
    if !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).context("failed to create log dir")?;
    }
    Ok(path.join("time_log"))
}

pub struct ActivityLog {
    file: File,
}

impl ActivityLog {
    pub fn load() -> Result<Self> {
        let path = activity_log_path().context("failed to open time log file")?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .context("failed to open time log file")?;
        let mut log = Self { file };
        log.log(Event::Power(true))?;
        Ok(log)
    }

    pub fn log(&mut self, event: Event) -> Result<()> {
        let timestamp = get_unix_time();
        writeln!(self.file, "{timestamp} {event}")?;

        Ok(())
    }
}

impl Drop for ActivityLog {
    fn drop(&mut self) {
        log::info!("saving activity log");
        self.log(Event::Power(false)).unwrap();
    }
}

pub struct ActivityRead {
    file: File,
}

impl ActivityRead {
    pub fn load() -> Result<Self> {
        let path = activity_log_path().context("failed to open time log file")?;
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .context("failed to open time log file")?;
        Ok(Self { file })
    }

    pub fn read(&mut self) -> Result<Vec<TimedEvent>> {
        let mut contents = String::new();
        self.file
            .read_to_string(&mut contents)
            .context("failed to read time log")?;
        contents
            .lines()
            .map(str::parse)
            .collect::<Result<Vec<TimedEvent>>>()
            .context("failed to parse time log")
    }
}
