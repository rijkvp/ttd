use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display},
    path::PathBuf,
};

pub mod socket;

pub const APP_NAME: &str = env!("CARGO_PKG_NAME");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: String,
    #[serde(rename = "type")]
    pub activity_type: ActivityType,
}

impl Display for Activity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.id, self.activity_type)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    Work,
    Break,
    Sleep,
}

impl Display for ActivityType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Work => write!(f, "work"),
            Self::Break => write!(f, "break"),
            Self::Sleep => write!(f, "sleep"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    GetList,
    Switch(String),
}

pub fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .expect("No runtime directory found!")
        .join(APP_NAME)
        .join(APP_NAME)
        .with_extension("sock")
}
