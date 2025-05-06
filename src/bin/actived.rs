/// actived is a daemon that deters if a user is 'active' or not by listening to input events.
/// It is indented to be ran seperately since it needs root permissions to access input devices.
use anyhow::{Result, bail};
use evdev::{Device, EventType};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Sender},
    },
    thread,
};
use tokio_stream::{StreamExt, StreamMap};
use ttd::{get_unix_time, socket::SocketServer};

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let (event_tx, event_rx) = mpsc::channel();

    let last_input = Arc::new(AtomicU64::new(get_unix_time()));
    {
        let last_input = last_input.clone();
        thread::spawn(move || {
            start_listener(event_tx);
        });
        thread::spawn(move || {
            loop {
                if event_rx.recv().is_ok() {
                    let timestamp = get_unix_time();
                    log::trace!("input event received at {timestamp} ");
                    last_input.store(timestamp, Ordering::Relaxed);
                }
            }
        });
    }

    let mut socket = SocketServer::create(ttd::activity_daemon_socket(), true)?;
    loop {
        let msg = bincode::encode_to_vec(
            last_input.load(Ordering::Relaxed),
            bincode::config::standard(),
        )?;
        socket.send(&msg)?;
    }
}

enum InputEvent {
    Keyboard,
    Mouse,
}

fn start_listener(event_tx: Sender<InputEvent>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        if let Err(e) = run_input_listener(event_tx).await {
            log::error!("failed to run event listener: {e}");
        }
    });
}

async fn run_input_listener(event_tx: Sender<InputEvent>) -> Result<()> {
    let devices: Vec<Device> = evdev::enumerate()
        .map(|(_, device)| device)
        .filter(|d| {
            // Filter on keyboard, mouse & touchscreen devices
            let supported = d.supported_events();
            supported.contains(EventType::KEY)
                || supported.contains(EventType::RELATIVE)
                || supported.contains(EventType::ABSOLUTE)
        })
        .collect();
    if devices.is_empty() {
        bail!("no input devices found! are you running as root?")
    }
    log::info!("listening for events on {} input devices", devices.len());
    let mut streams = StreamMap::new();
    for (n, device) in devices.into_iter().enumerate() {
        streams.insert(n, device.into_event_stream()?);
    }
    while let Some((_, Ok(event))) = streams.next().await {
        let event = match event.event_type() {
            EventType::KEY => Some(InputEvent::Keyboard),
            EventType::RELATIVE | EventType::ABSOLUTE => Some(InputEvent::Mouse),
            _ => None,
        };
        if let Some(event) = event {
            if let Err(e) = event_tx.send(event) {
                bail!("failed to send event: {e}");
            }
        }
    }
    Ok(())
}
