pub mod socket;

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

pub const APP_NAME: &str = env!("CARGO_PKG_NAME");

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub enum IpcMessage {
    Status,
    Switch(Option<Activity>),
    List,
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
