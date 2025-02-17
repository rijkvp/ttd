use anyhow::Result;
use clap::{command, Parser};
use ttd::{socket::SocketClient, Activity, Message};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum Command {
    /// List all available activities
    List,
    /// Switch to a new activity
    Switch { key: String },
}

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let args = Args::parse();
    let mut client = SocketClient::connect(ttd::socket_path())?;
    match args.cmd {
        Command::List => {
            let msg = Message::GetList;
            let bytes: Vec<u8> = rmp_serde::to_vec(&msg)?;
            let response = client.send(&bytes)?;
            let activities: Vec<Activity> = rmp_serde::from_read(response.as_slice())?;
            for activity in activities {
                println!("{}", activity);
            }
        }
        Command::Switch { key } => {
            let msg = Message::Switch(key);
            let bytes: Vec<u8> = rmp_serde::to_vec(&msg)?;
            let _ = client.send(&bytes)?;
        }
    };

    Ok(())
}
