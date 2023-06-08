use anyhow::{anyhow, bail, Result};
use thiserror::Error;

use crate::utils;

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

const CPC_INIT_TIMEOUT_MS: u128 = 2000;
const ENDPOINT_INIT_TIMEOUT_MS: u128 = 2000;
const ENDPOINT_RX_TIMEOUT_MS: u128 = 2000;

const CPC_READ_FLAGS: [libcpc::cpc_endpoint_read_flags_t_enum; 1] =
    [libcpc::cpc_endpoint_read_flags_t_enum::CPC_ENDPOINT_READ_FLAG_NONE];

const CPC_WRITE_FLAGS: [libcpc::cpc_endpoint_write_flags_t_enum; 1] =
    [libcpc::cpc_endpoint_write_flags_t_enum::CPC_ENDPOINT_WRITE_FLAG_NONE];

const CPC_TX_WINDOW_SIZE: u8 = 1;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Timeout({0}: {1} ms)")]
    Timeout(std::sync::mpsc::RecvTimeoutError, u128),
    #[error("Deserializer({0})")]
    Deserialization(anyhow::Error),
    #[error("Serializer({0})")]
    Serialization(anyhow::Error),
    #[error("Libcpc({0})")]
    Libcpc(#[from] libcpc::Error),
    #[error("Status({0})")]
    Packet(#[from] packet::Status),
}

pub struct GpioChip {
    pub unique_id: u64,
    pub chip_label: String,
    pub gpio_names: Vec<String>,
}

pub struct Endpoint {
    cpc_endpoint: libcpc::cpc_endpoint,
    ep_rx_channel: std::sync::mpsc::Receiver<Vec<u8>>,
    seq: u8,
    pub exit_signal: mio::unix::pipe::Receiver,
    pub gpio_chip: GpioChip,
}

impl Endpoint {
    pub fn new(instance_name: &str, enable_tracing: bool) -> Result<Self> {
        unsafe extern "C" fn reset_callback() {
            utils::exit(anyhow!("CPC reset callback"));
        }

        let now = std::time::Instant::now();
        let cpc_handle = loop {
            match libcpc::init(instance_name, enable_tracing, Some(reset_callback)) {
                Ok(cpc_handle) => {
                    log::info!("Initialized CPCd ({})", instance_name);
                    break cpc_handle;
                }
                Err(err) => {
                    if now.elapsed().as_millis() >= CPC_INIT_TIMEOUT_MS {
                        bail!("Is CPCd running? cpc_init({}), Err: {}", instance_name, err);
                    }
                }
            };
        };

        let ep_id = libcpc::cpc_endpoint_id::Service(
            libcpc::sl_cpc_service_endpoint_id_t_enum::SL_CPC_ENDPOINT_GPIO,
        );

        let now = std::time::Instant::now();
        let cpc_endpoint = loop {
            match cpc_handle.open_endpoint(ep_id, CPC_TX_WINDOW_SIZE) {
                Ok(cpc_endpoint) => {
                    log::info!("Initialized CPC Endpoint ({:?})", ep_id);
                    break cpc_endpoint;
                }
                Err(err) => {
                    if now.elapsed().as_millis() >= ENDPOINT_INIT_TIMEOUT_MS {
                        bail!("Failed to initialize CPC Endpoint, Err: {}", err);
                    }
                }
            };
        };

        let (ep_tx_channel, ep_rx_channel) = std::sync::mpsc::channel();
        let (mut ep_tx_pipe, ep_rx_pipe) = mio::unix::pipe::new()?;

        std::thread::spawn(move || loop {
            let buffer = match cpc_endpoint.read(&CPC_READ_FLAGS) {
                Ok(buffer) => buffer,
                Err(err) => {
                    let err_read = format!("Failed to read from endpoint, Err: {}", err);
                    if let Err(err_signal) =
                        std::io::Write::write(&mut ep_tx_pipe, &err_read.as_bytes())
                    {
                        utils::exit(anyhow!(
                            "{}, Failed to write to endpoint pipe, Err: {}",
                            err_read,
                            err_signal
                        ));
                    }
                    return;
                }
            };

            let packets = match packet::split(&buffer) {
                Ok(packets) => packets,
                Err(err) => {
                    log::warn!("Failed to split buffer: {:?}, Err: {}", buffer, err);
                    continue;
                }
            };

            for packet in packets {
                let cmd = packet::try_deserialize_cmd(&packet);

                match cmd {
                    Ok(rx_cmd) => match rx_cmd {
                        packet::SecondaryCmd::VersionIs
                        | packet::SecondaryCmd::StatusIs
                        | packet::SecondaryCmd::GpioCountIs
                        | packet::SecondaryCmd::GpioNameIs
                        | packet::SecondaryCmd::GpioValueIs
                        | packet::SecondaryCmd::ChipLabelIs
                        | packet::SecondaryCmd::UniqueIdIs => {
                            if let Err(err) = ep_tx_channel.send(packet) {
                                utils::exit(err.into());
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
                        log::warn!("Unknown packet received: {:?}, Err: {}", packet, err);
                    }
                }
            }
        });

        let gpio_chip = GpioChip {
            unique_id: 0,
            gpio_names: vec![],
            chip_label: String::new(),
        };

        let mut handle = Self {
            cpc_endpoint,
            ep_rx_channel,
            exit_signal: ep_rx_pipe,
            seq: 0,
            gpio_chip,
        };

        let secondary_version = handle.get_gpio_version()?;

        if VERSION.major != secondary_version.major {
            bail!(
                "Bridge Endpoint API (v{}) is not compatible with Secondary Endpoint API (v{})",
                VERSION,
                secondary_version
            );
        }

        handle.gpio_chip.unique_id = handle.get_unique_id()?;

        handle.gpio_chip.chip_label = handle.get_chip_label()?;

        let gpio_count = handle.get_gpio_count()?;

        for pin in 0..gpio_count {
            let name = handle.get_gpio_name(pin)?;
            handle.gpio_chip.gpio_names.push(name);
        }

        for pin in 0..gpio_count {
            handle.set_gpio_direction(pin, packet::GpioDirection::Disabled)?;
        }

        Ok(handle)
    }

    pub fn get_gpio_value(&mut self, pin: u8) -> Result<packet::GpioValueIs, Error> {
        let packet = packet::GetGpioValue::new(&mut self.seq, pin)
            .serialize()
            .map_err(Error::Serialization)?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let packet = self.read(Some(self.seq))?;

        let packet = packet::GpioValueIs::deserialize(&packet).map_err(Error::Deserialization)?;

        Ok(packet)
    }

    pub fn set_gpio_value(&mut self, pin: u8, value: packet::GpioValue) -> Result<(), Error> {
        let packet = packet::SetGpioValue::new(&mut self.seq, pin, value)
            .serialize()
            .map_err(Error::Serialization)?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let _packet = self.read(Some(self.seq))?;

        Ok(())
    }

    pub fn set_gpio_config(&mut self, pin: u8, config: packet::GpioConfig) -> Result<(), Error> {
        let packet = packet::SetGpioConfig::new(&mut self.seq, pin, config)
            .serialize()
            .map_err(Error::Serialization)?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let _packet = self.read(Some(self.seq))?;

        Ok(())
    }

    pub fn set_gpio_direction(
        &mut self,
        pin: u8,
        direction: packet::GpioDirection,
    ) -> Result<(), Error> {
        let packet = packet::SetGpioDirection::new(&mut self.seq, pin, direction)
            .serialize()
            .map_err(Error::Serialization)?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let _packet = self.read(Some(self.seq))?;

        Ok(())
    }

    pub fn read_exit_signal(&mut self) -> Result<String> {
        let mut data = String::new();
        std::io::Read::read_to_string(&mut self.exit_signal, &mut data)?;
        Ok(data)
    }
}

impl Endpoint {
    fn get_gpio_version(&mut self) -> Result<utils::Version> {
        let packet = packet::GetVersion::new().serialize()?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let packet = self.read(None)?;
        let packet = packet::VersionIs::deserialize(&packet)?;

        Ok(packet.version)
    }

    fn get_unique_id(&mut self) -> Result<u64> {
        let packet = packet::GetUniqueId::new(&mut self.seq).serialize()?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let packet = self.read(Some(self.seq))?;
        let packet = packet::UniqueIdIs::deserialize(&packet)?;

        Ok(packet.unique_id)
    }

    fn get_chip_label(&mut self) -> Result<String> {
        let packet = packet::GetChipLabel::new(&mut self.seq).serialize()?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let packet = self.read(Some(self.seq))?;
        let packet = packet::ChipLabelIs::deserialize(&packet)?;

        packet.chip_label
    }

    fn get_gpio_count(&mut self) -> Result<u8> {
        let packet = packet::GetGpioCount::new(&mut self.seq).serialize()?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let packet = self.read(Some(self.seq))?;
        let packet = packet::GpioCountIs::deserialize(&packet)?;

        Ok(packet.count)
    }

    fn get_gpio_name(&mut self, pin: u8) -> Result<String> {
        let packet = packet::GetGpioName::new(&mut self.seq, pin).serialize()?;

        self.cpc_endpoint.write(&packet, &CPC_WRITE_FLAGS)?;

        let packet = self.read(Some(self.seq))?;
        let packet = packet::GpioNameIs::deserialize(&packet)?;

        packet.name
    }

    fn read(&self, expected_seq: Option<u8>) -> Result<Vec<u8>, Error> {
        let now = std::time::Instant::now();
        let mut timeout = ENDPOINT_RX_TIMEOUT_MS;
        loop {
            match self
                .ep_rx_channel
                .recv_timeout(core::time::Duration::from_millis(timeout as u64))
            {
                Ok(packet) => {
                    if let Some(expected_seq) = expected_seq {
                        let (header, rx_header) = packet::deserialize_headers(&packet)
                            .map_err(|err| Error::Deserialization(anyhow!(err.to_string())))?
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
                                .map_err(Error::Deserialization)?;
                            if status.status != Status::Ok {
                                return Err(Error::Packet(status.status));
                            }
                        }
                    }

                    return Ok(packet);
                }
                Err(err) => match err {
                    std::sync::mpsc::RecvTimeoutError::Timeout => {
                        let elapsed = now.elapsed().as_millis();
                        if elapsed >= timeout {
                            return Err(Error::Timeout(err, elapsed));
                        } else {
                            timeout -= elapsed;
                        }
                    }
                    std::sync::mpsc::RecvTimeoutError::Disconnected => {
                        utils::exit(err.into());
                    }
                },
            };
        }
    }
}
