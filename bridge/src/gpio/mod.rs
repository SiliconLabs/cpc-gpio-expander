use anyhow::{anyhow, bail, Result};
use std::sync::Mutex;
use std::sync::{mpsc, Arc};
use thiserror::Error;

use crate::utils;

mod interface;

mod packet;
use self::packet::Serializer;
pub use packet::GpioConfig;
pub use packet::GpioDirection;
pub use packet::GpioValue;
pub use packet::Status;

pub const VERSION: utils::Version = utils::Version {
    major: 1,
    minor: 0,
    patch: 0,
};

const READ_TIMEOUT_MS: u128 = 2000;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Recoverable(#[from] RecoverableError),
    #[error(transparent)]
    Unrecoverable(#[from] UnrecoverableError),
}

#[derive(Error, Debug)]
pub enum RecoverableError {
    #[error("Timeout({0}: {1} ms)")]
    Timeout(mpsc::RecvTimeoutError, u128),
    #[error("Deserializer({0})")]
    Deserialization(anyhow::Error),
    #[error("Serializer({0})")]
    Serialization(anyhow::Error),
    #[error("Status({0})")]
    Packet(packet::Status),
}

#[derive(Error, Debug)]
pub enum UnrecoverableError {
    #[error(transparent)]
    Interface(interface::Error),
    #[error(transparent)]
    Anyhow(anyhow::Error),
}

pub trait Gpio {
    fn write(&self, bytes: &[u8]) -> Result<(), Error>;
    fn read(&self) -> Result<Vec<u8>, Error>;
}
pub type GpioTraits = dyn Gpio + Send + Sync;

pub struct Chip {
    pub unique_id: u64,
    pub label: String,
    pub gpio_names: Vec<String>,
}

pub struct Handle {
    pub exit: utils::ThreadExit,
    pub chip: Chip,
    gpio: Arc<Box<GpioTraits>>,
    data_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    seq: Mutex<u8>,
}

impl Handle {
    pub fn new(config: &utils::Config, trace_config: &utils::TraceConfig) -> Result<Self> {
        let interface = interface::new(config, trace_config)?;
        let gpio = Arc::new(interface);
        let gpio_ref = gpio.clone();

        let (data_tx, data_rx) = mpsc::channel();
        let (mut exit_sender, exit_receiver) = mio::unix::pipe::new()?;

        std::thread::Builder::new()
            .name("gpio".to_string())
            .spawn(move || loop {
                let result = (|| -> Result<()> {
                    let buffer = match gpio_ref.read() {
                        Ok(buffer) => buffer,
                        Err(err) => bail!("Failed to read from GPIO, Err: {:?}", err),
                    };

                    match packet::split(&buffer) {
                        Ok(packets) => {
                            for packet in packets {
                                match packet::try_deserialize_cmd(&packet) {
                                    Ok(rx_cmd) => match rx_cmd {
                                        packet::SecondaryCmd::VersionIs
                                        | packet::SecondaryCmd::StatusIs
                                        | packet::SecondaryCmd::GpioCountIs
                                        | packet::SecondaryCmd::GpioNameIs
                                        | packet::SecondaryCmd::GpioValueIs
                                        | packet::SecondaryCmd::ChipLabelIs
                                        | packet::SecondaryCmd::UniqueIdIs => {
                                            if let Err(err) = data_tx.send(packet) {
                                                bail!(
                                                    "Failed to send to GPIO channel, Err: {}",
                                                    err
                                                )
                                            }
                                        }
                                        packet::SecondaryCmd::UnsupportedCmdIs => {
                                            match packet::UnsupportedCmdIs::deserialize(&packet) {
                                                Ok(packet) => log::warn!("{:?}", packet),
                                                Err(err) => {
                                                    log::warn!(
                                                    "Unable to deserialize packet: {:?}, Err: {}",
                                                    packet,
                                                    err
                                                )
                                                }
                                            }
                                        }
                                    },
                                    Err(err) => {
                                        log::warn!(
                                            "Unknown packet received: {:?}, Err: {}",
                                            packet,
                                            err
                                        );
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            log::warn!("Failed to split buffer: {:?}, Err: {}", buffer, err);
                        }
                    };

                    Ok(())
                })();

                if let Err(err) = result {
                    utils::ThreadExit::notify(&mut exit_sender, &format!("{}", err));
                    return;
                }
            })?;

        let chip = Chip {
            unique_id: 0,
            gpio_names: vec![],
            label: String::new(),
        };

        let mut handle = Self {
            exit: utils::ThreadExit {
                receiver: Mutex::new(exit_receiver),
            },
            chip,
            gpio,
            data_rx: Mutex::new(data_rx),
            seq: Mutex::new(0),
        };

        let gpio_version = handle.get_gpio_version()?;

        if VERSION.major != gpio_version.major {
            bail!(
                "Bridge GPIO API (v{}) is not compatible with GPIO API (v{})",
                VERSION,
                gpio_version
            );
        }

        handle.chip.unique_id = handle.get_unique_id()?;

        handle.chip.label = handle.get_chip_label()?;

        let gpio_count = handle.get_gpio_count()?;

        for pin in 0..gpio_count {
            let name = handle.get_gpio_name(pin)?;
            handle.chip.gpio_names.push(name);
        }

        for pin in 0..gpio_count {
            handle.set_gpio_direction(pin, packet::GpioDirection::Disabled)?;
        }

        Ok(handle)
    }

    pub fn get_gpio_value(&self, pin: u8) -> Result<packet::GpioValueIs, Error> {
        let (packet, expected_seq) = {
            let mut seq = self
                .seq
                .lock()
                .map_err(|err| UnrecoverableError::Anyhow(anyhow!("{}", err)))?;

            let packet = packet::GetGpioValue::new(&mut seq, pin)
                .serialize()
                .map_err(RecoverableError::Serialization)?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let packet = self.read(Some(expected_seq))?;

        let packet =
            packet::GpioValueIs::deserialize(&packet).map_err(RecoverableError::Deserialization)?;

        Ok(packet)
    }

    pub fn set_gpio_value(&self, pin: u8, value: packet::GpioValue) -> Result<(), Error> {
        let (packet, expected_seq) = {
            let mut seq = self
                .seq
                .lock()
                .map_err(|err| UnrecoverableError::Anyhow(anyhow!("{}", err)))?;

            let packet = packet::SetGpioValue::new(&mut seq, pin, value)
                .serialize()
                .map_err(RecoverableError::Serialization)?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let _packet = self.read(Some(expected_seq))?;

        Ok(())
    }

    pub fn set_gpio_config(&self, pin: u8, config: packet::GpioConfig) -> Result<(), Error> {
        let (packet, expected_seq) = {
            let mut seq = self
                .seq
                .lock()
                .map_err(|err| UnrecoverableError::Anyhow(anyhow!("{}", err)))?;

            let packet = packet::SetGpioConfig::new(&mut seq, pin, config)
                .serialize()
                .map_err(RecoverableError::Serialization)?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let _packet = self.read(Some(expected_seq))?;

        Ok(())
    }

    pub fn set_gpio_direction(
        &self,
        pin: u8,
        direction: packet::GpioDirection,
    ) -> Result<(), Error> {
        let (packet, expected_seq) = {
            let mut seq = self
                .seq
                .lock()
                .map_err(|err| UnrecoverableError::Anyhow(anyhow!("{}", err)))?;

            let packet = packet::SetGpioDirection::new(&mut seq, pin, direction)
                .serialize()
                .map_err(RecoverableError::Serialization)?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let _packet = self.read(Some(expected_seq))?;

        Ok(())
    }
}

impl Handle {
    fn get_gpio_version(&self) -> Result<utils::Version> {
        let packet = packet::GetVersion::new().serialize()?;

        self.gpio.write(&packet)?;

        let packet = self.read(None)?;
        let packet = packet::VersionIs::deserialize(&packet)?;

        Ok(packet.version)
    }

    fn get_unique_id(&self) -> Result<u64> {
        let (packet, expected_seq) = {
            let mut seq = self.seq.lock().map_err(|err| anyhow!("{}", err))?;

            let packet = packet::GetUniqueId::new(&mut seq).serialize()?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let packet = self.read(Some(expected_seq))?;
        let packet = packet::UniqueIdIs::deserialize(&packet)?;

        Ok(packet.unique_id)
    }

    fn get_chip_label(&self) -> Result<String> {
        let (packet, expected_seq) = {
            let mut seq = self.seq.lock().map_err(|err| anyhow!("{}", err))?;

            let packet = packet::GetChipLabel::new(&mut seq).serialize()?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let packet = self.read(Some(expected_seq))?;
        let packet = packet::ChipLabelIs::deserialize(&packet)?;

        packet.chip_label
    }

    fn get_gpio_count(&self) -> Result<u8> {
        let (packet, expected_seq) = {
            let mut seq = self.seq.lock().map_err(|err| anyhow!("{}", err))?;

            let packet = packet::GetGpioCount::new(&mut seq).serialize()?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let packet = self.read(Some(expected_seq))?;
        let packet = packet::GpioCountIs::deserialize(&packet)?;

        Ok(packet.count)
    }

    fn get_gpio_name(&self, pin: u8) -> Result<String> {
        let (packet, expected_seq) = {
            let mut seq = self.seq.lock().map_err(|err| anyhow!("{}", err))?;

            let packet = packet::GetGpioName::new(&mut seq, pin).serialize()?;

            (packet, seq.clone())
        };

        self.gpio.write(&packet)?;

        let packet = self.read(Some(expected_seq))?;
        let packet = packet::GpioNameIs::deserialize(&packet)?;

        packet.name
    }

    fn read(&self, expected_seq: Option<u8>) -> Result<Vec<u8>, Error> {
        let now = std::time::Instant::now();
        let mut timeout = READ_TIMEOUT_MS;
        loop {
            match self
                .data_rx
                .lock()
                .map_err(|err| UnrecoverableError::Anyhow(anyhow!("{}", err)))?
                .recv_timeout(core::time::Duration::from_millis(timeout as u64))
            {
                Ok(packet) => {
                    if let Some(expected_seq) = expected_seq {
                        let (header, rx_header) = packet::deserialize_headers(&packet)
                            .map_err(|err| {
                                RecoverableError::Deserialization(anyhow!(err.to_string()))
                            })?
                            .1;

                        if expected_seq != rx_header.seq {
                            log::warn!(
                                "{:?} {{ Sequence number mismatch (Expected: {}, Received: {}) }}",
                                header.cmd,
                                expected_seq,
                                rx_header.seq,
                            );
                            continue;
                        }

                        if let packet::SecondaryCmd::StatusIs = header.cmd {
                            let status = packet::StatusIs::deserialize(&packet)
                                .map_err(RecoverableError::Deserialization)?;
                            if status.status != Status::Ok {
                                return Err(RecoverableError::Packet(status.status).into());
                            }
                        }
                    }

                    return Ok(packet);
                }
                Err(err) => match err {
                    mpsc::RecvTimeoutError::Timeout => {
                        let elapsed = now.elapsed().as_millis();
                        if elapsed >= timeout {
                            return Err(RecoverableError::Timeout(err, elapsed).into());
                        } else {
                            timeout -= elapsed;
                        }
                    }
                    mpsc::RecvTimeoutError::Disconnected => {
                        return Err(UnrecoverableError::Anyhow(anyhow!(
                            "{}",
                            mpsc::RecvTimeoutError::Disconnected
                        ))
                        .into());
                    }
                },
            };
        }
    }
}
