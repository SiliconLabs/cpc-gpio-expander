use anyhow::{anyhow, bail, Context, Result};
use neli::{
    consts::{
        nl::{NlmF, NlmFFlags},
        socket::NlFamily,
    },
    genl::{Genlmsghdr, Nlattr},
    nl::{NlPayload, Nlmsghdr},
    socket::NlSocketHandle,
    types::{Buffer, GenlBuffer},
};
use std::sync::{mpsc, Mutex};

mod packet;
pub use packet::Exit;
pub use packet::GetGpioValue;
pub use packet::GpioConfig;
pub use packet::GpioDirection;
pub use packet::GpioValue;
pub use packet::Packet;
pub use packet::SetGpioConfig;
pub use packet::SetGpioDirection;
pub use packet::SetGpioValue;
pub use packet::Status;

use crate::utils;

pub const VERSION: utils::Version = utils::Version {
    major: 1,
    minor: 0,
    patch: 0,
};

const GENL_API_VERSION: u8 = 1;
const GENL_FAMILY_NAME: &str = "CPC_GPIO_GENL";
const GENL_MULTICAST_FAMILY_NAME: &str = "CPC_GPIO_GENL_M";
const GENL_MULTICAST_UID_ALL: u64 = 0;

pub struct Handle {
    pub exit: utils::ThreadExit,
    data_rx: Mutex<mpsc::Receiver<Nlmsghdr<u16, Genlmsghdr<packet::Command, packet::Attribute>>>>,
    unicast: Mutex<NlSocketHandle>,
    family_id: u16,
}

impl Handle {
    pub fn new(
        deinit_and_exit: bool,
        unique_id: u64,
        chip_label: &str,
        names: &Vec<String>,
    ) -> Result<Self> {
        // Connect to generic netlink unicast
        let mut unicast = NlSocketHandle::connect(NlFamily::Generic, Some(0), &[])?;

        let family_id = match unicast.resolve_genl_family(GENL_FAMILY_NAME) {
            Ok(family_id) => family_id,
            Err(err) => {
                bail!(
                    "The Generic Netlink family ({}) can't be found. Is the Kernel Driver loaded? Err: {}",
                    GENL_FAMILY_NAME,
                    err);
            }
        };

        let multicast_group =
            match unicast.resolve_nl_mcast_group(GENL_FAMILY_NAME, GENL_MULTICAST_FAMILY_NAME) {
                Ok(multicast_group) => multicast_group,
                Err(err) => {
                    bail!(
                        "Failed to resolve using Generic Netlink ({}) Multicast ({}), Err: {}",
                        GENL_FAMILY_NAME,
                        GENL_MULTICAST_FAMILY_NAME,
                        err,
                    );
                }
            };

        // Connect to generic netlink multicast
        let mut multicast =
            NlSocketHandle::connect(NlFamily::Generic, Some(0), &[multicast_group])?;

        let (data_tx, data_rx) = std::sync::mpsc::channel::<
            Nlmsghdr<u16, Genlmsghdr<packet::Command, packet::Attribute>>,
        >();

        let (mut exit_sender, exit_receiver) = mio::unix::pipe::new()?;

        std::thread::Builder::new()
            .name("driver".to_string())
            .spawn(move || loop {
                let result = (|| -> Result<()> {
                    let packet = match multicast.recv() {
                        Ok(packet) => packet.context("Multicast socked was closed")?,
                        Err(err) => bail!("Failed to read from Multicast socket, Err: {}", err),
                    };

                    let filtered = match filter_packet(unique_id, &packet) {
                        Ok(filtered) => filtered,
                        Err(err) => bail!("Failed to filter packet, Err: {}", err),
                    };

                    if !filtered {
                        if let Err(err) = data_tx.send(packet) {
                            bail!("Failed to send to Driver channel, Err: {}", err)
                        }
                    }

                    Ok(())
                })();

                if let Err(err) = result {
                    utils::ThreadExit::notify(&mut exit_sender, &format!("{}", err));
                    return;
                }
            })?;

        let handle = Self {
            exit: utils::ThreadExit {
                receiver: Mutex::new(exit_receiver),
            },
            data_rx: Mutex::new(data_rx),
            unicast: Mutex::new(unicast),
            family_id,
        };

        handle.deinit(unique_id)?;

        if deinit_and_exit {
            bail!(utils::ProcessExit::Context(anyhow!(
                "Deinitialized Kernel Driver (UID: {})",
                unique_id
            )));
        }

        handle.init(unique_id, chip_label, names)?;

        Ok(handle)
    }

    pub fn get_gpio_value_reply(
        &self,
        unique_id: u64,
        gpio_pin: u32,
        gpio_value: Option<u32>,
        status: Option<packet::Status>,
    ) -> Result<()> {
        if let Some(status) = status {
            let mut attributes = GenlBuffer::new();

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::UniqueId,
                unique_id,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::GpioPin,
                gpio_pin,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::Status,
                status as u32,
            )?);

            if let Some(gpio_value) = gpio_value {
                attributes.push(Nlattr::new(
                    false,
                    false,
                    packet::Attribute::GpioValue,
                    gpio_value,
                )?);
            }

            self.send(packet::Command::GetGpioValue, attributes)?;
        }

        Ok(())
    }

    pub fn set_gpio_value_reply(
        &self,
        unique_id: u64,
        gpio_pin: u32,
        status: Option<packet::Status>,
    ) -> Result<()> {
        if let Some(status) = status {
            let mut attributes = GenlBuffer::new();

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::UniqueId,
                unique_id,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::GpioPin,
                gpio_pin,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::Status,
                status as u32,
            )?);

            self.send(packet::Command::SetGpioValue, attributes)?;
        }

        Ok(())
    }

    pub fn set_gpio_config_reply(
        &self,
        unique_id: u64,
        gpio_pin: u32,
        status: Option<packet::Status>,
    ) -> Result<()> {
        if let Some(status) = status {
            let mut attributes = GenlBuffer::new();

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::UniqueId,
                unique_id,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::GpioPin,
                gpio_pin,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::Status,
                status as u32,
            )?);

            self.send(packet::Command::SetGpioConfig, attributes)?;
        }

        Ok(())
    }

    pub fn set_gpio_direction_reply(
        &self,
        unique_id: u64,
        gpio_pin: u32,
        status: Option<packet::Status>,
    ) -> Result<()> {
        if let Some(status) = status {
            let mut attributes = GenlBuffer::new();

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::UniqueId,
                unique_id,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::GpioPin,
                gpio_pin,
            )?);

            attributes.push(Nlattr::new(
                false,
                false,
                packet::Attribute::Status,
                status as u32,
            )?);

            self.send(packet::Command::SetGpioDirection, attributes)?;
        }

        Ok(())
    }

    pub fn deinit(&self, unique_id: u64) -> Result<()> {
        let mut attributes = GenlBuffer::new();

        attributes.push(Nlattr::new(
            false,
            false,
            packet::Attribute::UniqueId,
            unique_id,
        )?);

        self.send(packet::Command::Deinit, attributes)?;

        let packet = self.read_sync()?;
        let payload = packet
            .nl_payload
            .get_payload()
            .context("No payload from Kernel Driver")?;
        let genl_version = payload.version;

        if GENL_API_VERSION != genl_version {
            bail!(
                "Bridge Driver Generic Netlink API (v{}) != Kernel Driver Generic Netlink API (v{})",
                GENL_API_VERSION, genl_version
            );
        }

        let attributes = payload.get_attr_handle();

        let driver_version = utils::Version {
            major: attributes.get_attr_payload_as::<u8>(packet::Attribute::VersionMajor)?,
            minor: attributes.get_attr_payload_as::<u8>(packet::Attribute::VersionMinor)?,
            patch: attributes.get_attr_payload_as::<u8>(packet::Attribute::VersionPatch)?,
        };

        if VERSION.major != driver_version.major {
            bail!(
                "Bridge Driver API (v{}) is not compatible with Kernel Driver API (v{})",
                VERSION,
                driver_version
            );
        }

        let status = attributes.get_attr_payload_as::<u32>(packet::Attribute::Status)?;
        if status != 0 {
            bail!(
                "Failed to deinitialize Kernel Driver, Err: {}",
                std::io::Error::from_raw_os_error(status as i32)
            );
        }

        Ok(())
    }

    pub fn read(&self) -> Result<Nlmsghdr<u16, Genlmsghdr<packet::Command, packet::Attribute>>> {
        Ok(self
            .data_rx
            .lock()
            .map_err(|err| anyhow!("{}", err))?
            .recv()?)
    }

    pub fn parse(
        &self,
        packet: Nlmsghdr<u16, Genlmsghdr<packet::Command, packet::Attribute>>,
    ) -> Result<packet::Packet> {
        let attributes = packet.get_payload()?.get_attr_handle();
        let payload = packet
            .nl_payload
            .get_payload()
            .context("No payload from Kernel Driver")?;

        match payload.cmd {
            packet::Command::Exit => {
                let message = attributes
                    .get_attr_payload_as_with_len::<String>(packet::Attribute::Message)?;

                Ok(packet::Packet::Exit(packet::Exit { message }))
            }
            packet::Command::GetGpioValue => {
                let pin = attributes.get_attr_payload_as::<u32>(packet::Attribute::GpioPin)?;

                Ok(packet::Packet::GetGpioValue(packet::GetGpioValue { pin }))
            }
            packet::Command::SetGpioValue => {
                let pin = attributes.get_attr_payload_as::<u32>(packet::Attribute::GpioPin)?;

                let value = attributes.get_attr_payload_as::<u32>(packet::Attribute::GpioValue)?;

                let value = packet::GpioValue::try_from(value)?;

                Ok(packet::Packet::SetGpioValue(packet::SetGpioValue {
                    pin,
                    value,
                }))
            }
            packet::Command::SetGpioConfig => {
                let pin = attributes.get_attr_payload_as::<u32>(packet::Attribute::GpioPin)?;

                let config =
                    attributes.get_attr_payload_as::<u32>(packet::Attribute::GpioConfig)?;

                let config = packet::GpioConfig::try_from(config)?;

                Ok(packet::Packet::SetGpioConfig(packet::SetGpioConfig {
                    pin,
                    config,
                }))
            }
            packet::Command::SetGpioDirection => {
                let pin = attributes.get_attr_payload_as::<u32>(packet::Attribute::GpioPin)?;

                let direction =
                    attributes.get_attr_payload_as::<u32>(packet::Attribute::GpioDirection)?;

                let direction = packet::GpioDirection::try_from(direction)?;

                Ok(packet::Packet::SetGpioDirection(packet::SetGpioDirection {
                    pin,
                    direction,
                }))
            }
            _ => {
                bail!("[{:#?}] Unknown command", payload.cmd);
            }
        }
    }
}

impl Handle {
    fn init(&self, unique_id: u64, label: &str, gpio_names: &Vec<String>) -> Result<()> {
        if unique_id == GENL_MULTICAST_UID_ALL {
            bail!("Unique ID cannot be {}", GENL_MULTICAST_UID_ALL);
        }

        if gpio_names.is_empty() {
            bail!("GPIO count cannot be {}", gpio_names.len());
        }

        let mut attributes = GenlBuffer::new();

        attributes.push(Nlattr::new(
            false,
            false,
            packet::Attribute::UniqueId,
            unique_id,
        )?);

        attributes.push(Nlattr::new(
            false,
            false,
            packet::Attribute::GpioCount,
            gpio_names.len() as u32,
        )?);

        attributes.push(Nlattr::new(
            false,
            false,
            packet::Attribute::GpioNames,
            gpio_names.clone(),
        )?);

        attributes.push(Nlattr::new(
            false,
            false,
            packet::Attribute::ChipLabel,
            label,
        )?);

        self.send(packet::Command::Init, attributes)?;

        let packet = self.read_sync()?;

        let attributes = packet.get_payload()?.get_attr_handle();

        let status = attributes.get_attr_payload_as::<u32>(packet::Attribute::Status)?;

        let args = format!(
            "UID: {:?}, Label: {:?}, GPIO's: {:?}",
            unique_id, label, gpio_names
        );

        if status != 0 {
            bail!(
                "Failed to initialize Kernel Driver ({}), Err: {}",
                args,
                std::io::Error::from_raw_os_error(status as i32)
            );
        } else {
            log::info!("Initialized Kernel Driver ({})", args);
        }

        Ok(())
    }

    fn read_sync(&self) -> Result<Nlmsghdr<u16, Genlmsghdr<packet::Command, packet::Attribute>>> {
        let buffer = self
            .unicast
            .lock()
            .map_err(|err| anyhow!("{}", err))?
            .recv()?;

        Ok(buffer.context("Nothing to read from Kernel Driver")?)
    }

    fn send(
        &self,
        cmd: packet::Command,
        attributes: GenlBuffer<packet::Attribute, Buffer>,
    ) -> Result<()> {
        let nlmsghdr = Nlmsghdr::new(
            None,
            self.family_id,
            NlmFFlags::new(&[NlmF::Request]),
            None,
            Some(std::process::id()),
            NlPayload::Payload(Genlmsghdr::new(cmd, GENL_API_VERSION, attributes)),
        );

        self.unicast
            .lock()
            .map_err(|err| anyhow!("{}", err))?
            .send(nlmsghdr)?;

        Ok(())
    }
}

fn filter_packet(
    unique_id: u64,
    packet: &Nlmsghdr<u16, Genlmsghdr<packet::Command, packet::Attribute>>,
) -> Result<bool> {
    let attributes = packet.get_payload()?.get_attr_handle();
    let destination = attributes.get_attr_payload_as::<u64>(packet::Attribute::UniqueId)?;

    match destination {
        GENL_MULTICAST_UID_ALL => Ok(false),
        destination if destination == unique_id => Ok(false),
        _ => Ok(true),
    }
}
