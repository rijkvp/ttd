use anyhow::Result;
use clap::{Parser, command};
use jiff::{SignedDuration, Timestamp, Zoned, tz::TimeZone};
use std::collections::BTreeMap;
use ttd::{Activity, ActivityRead, Event, IpcRequest, IpcResponse, async_socket::SocketStream};

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
    /// Get stattistics
    Stats,
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
            Command::Stats => {
                let events = ActivityRead::load()?.read()?;
                let start = Zoned::now()
                    .with()
                    .hour(0)
                    .minute(0)
                    .second(0)
                    .build()?
                    .timestamp()
                    .as_second();
                let end = Zoned::now()
                    .with()
                    .hour(23)
                    .minute(59)
                    .second(59)
                    .build()?
                    .timestamp()
                    .as_second();
                let mut totals: BTreeMap<Activity, i64> = BTreeMap::new();

                let mut prev: Option<Activity> = None;
                let mut prev_time = None;

                println!("Activities today:");
                for event in events {
                    if event.timestamp >= start && event.timestamp <= end {
                        match event.event {
                            Event::Power(state) => {
                                if !state {
                                    // activity ends on poweroff
                                    prev = None;
                                    prev_time = None;
                                }
                            }
                            Event::SwitchActivity(activity) => {
                                if let (Some(prev_activity), Some(prev_time)) = (prev, prev_time) {
                                    let duration = event.timestamp - prev_time;
                                    println!(
                                        "{} - {}\t{}\t{:#}",
                                        Timestamp::new(prev_time, 0)
                                            .unwrap()
                                            .to_zoned(TimeZone::system())
                                            .time(),
                                        Timestamp::new(event.timestamp, 0)
                                            .unwrap()
                                            .to_zoned(TimeZone::system())
                                            .time(),
                                        prev_activity,
                                        SignedDuration::new(duration, 0)
                                    );
                                    *totals.entry(prev_activity.clone()).or_insert(0) += duration;
                                }
                                prev = activity;
                                prev_time = Some(event.timestamp);
                            }
                        }
                    }
                }

                println!("\nActivity totals for today:");
                for (activity, duration) in totals {
                    println!("{}\t{:#}", activity, SignedDuration::new(duration, 0));
                }
            }
        };
        Ok(())
    }
}
