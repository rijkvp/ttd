use anyhow::Result;
use clap::{Parser, command};
use ttd::{Activity, IpcRequest, IpcResponse, async_socket::SocketStream};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let args = Args::parse();
    Client::connect().await?.run(args.cmd).await?;
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
    stream: SocketStream,
}

impl Client {
    async fn connect() -> Result<Self> {
        let stream = SocketStream::connect(ttd::socket_path()).await?;
        Ok(Self { stream })
    }

    async fn run(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::List => {
                if let IpcResponse::Activities(activities) =
                    self.stream.send_and_recv(IpcRequest::GetActivities).await?
                {
                    for activity in activities {
                        println!("{}", activity);
                    }
                }
            }
            Command::Switch { activity } => {
                if !matches!(
                    self.stream
                        .send_and_recv(IpcRequest::Switch(Some(Activity::new(activity)?)))
                        .await?,
                    IpcResponse::Empty
                ) {
                    eprintln!("unexpected response from server");
                }
            }
            Command::Status => {
                if let IpcResponse::Status(status) =
                    self.stream.send_and_recv(IpcRequest::Status).await?
                {
                    println!("{status}");
                }
            }
            Command::Stop => {
                if !matches!(
                    self.stream.send_and_recv(IpcRequest::Switch(None)).await?,
                    IpcResponse::Empty
                ) {
                    eprintln!("unexpected response from server");
                }
            }
        };
        Ok(())
    }
}
