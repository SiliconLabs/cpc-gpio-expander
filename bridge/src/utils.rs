use anyhow::{anyhow, Result};
use std::{
    io::{Read, Write},
    sync::Mutex,
};
use thiserror::Error;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}
impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, clap::ValueEnum)]
pub enum Trace {
    None,
    Bridge,
    Libcpc,
    All,
}

#[derive(clap::Parser, Debug)]
#[clap(version, about)]
pub struct Config {
    /// Enable tracing
    #[clap(short, long, value_enum, default_value_t = Trace::None)]
    pub trace: Trace,

    /// Name of the cpcd instance
    #[clap(short, long, default_value = "cpcd_0")]
    pub instance: String,

    /// Bridge lock directory
    #[clap(short, long, default_value = "/tmp")]
    pub lock_dir: String,

    /// Deinit gpio chip and exit process
    #[clap(short, long, default_value = "false")]
    pub deinit: bool,
}

pub struct TraceConfig {
    pub bridge: log::LevelFilter,
    pub libcpc: bool,
}

pub fn trace(config: &Config) -> TraceConfig {
    let mut trace_config = TraceConfig {
        bridge: log::LevelFilter::Info,
        libcpc: false,
    };

    match config.trace {
        Trace::None => (),
        Trace::Bridge => {
            trace_config.bridge = log::LevelFilter::Debug;
        }
        Trace::Libcpc => {
            trace_config.libcpc = true;
        }
        Trace::All => {
            trace_config.bridge = log::LevelFilter::Debug;
            trace_config.libcpc = true;
        }
    }

    trace_config
}

pub fn lock_bridge(path: &std::path::Path) -> Result<file_lock::FileLock> {
    let lock = if let Ok(lock) = file_lock::FileLock::lock(
        path,
        false,
        file_lock::FileOptions::new().create(true).append(true),
    ) {
        lock
    } else {
        file_lock::FileLock::lock(path, false, file_lock::FileOptions::new().append(true)).map_err(
            |err| {
                anyhow!(
                    "The bridge lock ({}) cannot be taken. Err: {}",
                    path.display(),
                    err
                )
            },
        )?
    };

    Ok(lock)
}

#[derive(Error, Debug)]
pub enum ProcessExit {
    #[error(transparent)]
    Context(anyhow::Error),
}
pub fn exit(err: anyhow::Error) -> ! {
    if let Some(context) = err.downcast_ref::<ProcessExit>() {
        log::info!("{}", context);
        std::process::exit(0);
    } else {
        log::error!("{}\nBacktrace:\n{}", err, err.backtrace());
        std::process::exit(1);
    }
}

#[derive(Debug)]
pub struct ThreadExit {
    pub receiver: Mutex<mio::unix::pipe::Receiver>,
}
impl ThreadExit {
    pub fn notify(sender: &mut mio::unix::pipe::Sender, message: &str) {
        if let Err(err) = sender.write_all(message.as_bytes()) {
            exit(anyhow!(
                "{}, Failed to notify thread exit, Err: {}",
                message,
                err
            ));
        }
    }
}
impl std::fmt::Display for ThreadExit {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut message = String::new();

        let message = match self.receiver.lock() {
            Ok(mut receiver) => match receiver.read_to_string(&mut message) {
                Ok(_) => message,
                Err(err) => err.to_string(),
            },
            Err(err) => err.to_string(),
        };

        write!(f, "{}", message)
    }
}
