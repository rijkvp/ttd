use anyhow::Result;
use clap::{Parser, command};
use ttd::{Activity, IpcMessage, Status, socket::SocketClient};

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let args = Args::parse();
    let mut client = Client::new()?;
    client.run(args.cmd)?;

    Ok(())
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum Command {
    /// Get the current activity
    Status,
    /// List all available activities
    List,
    /// Switch to a new activity
    Switch { activity: String },
    /// Stop tracking the current activity
    Stop,
}

struct Client {
    socket: SocketClient,
}

impl Client {
    fn new() -> Result<Self> {
        let socket = SocketClient::connect(ttd::socket_path())?;
        Ok(Self { socket })
    }

    fn run(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::List => {
                let activities: Vec<Activity> = self.send(IpcMessage::List)?;
                for activity in activities {
                    println!("{}", activity);
                }
            }
            Command::Switch { activity } => {
                let () = self.send(IpcMessage::Switch(Some(Activity::new(activity)?)))?;
            }
            Command::Status => {
                let status: Status = self.send(IpcMessage::Status)?;
                println!("{status}");
            }
            Command::Stop => {
                let () = self.send(IpcMessage::Switch(None))?;
            }
        };
        Ok(())
    }

    fn send<T>(&mut self, msg: IpcMessage) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let bytes: Vec<u8> = rmp_serde::to_vec(&msg)?;
        let response = self.socket.send(&bytes)?;
        let data: T = rmp_serde::from_read(response.as_slice())?;
        Ok(data)
    }
}
