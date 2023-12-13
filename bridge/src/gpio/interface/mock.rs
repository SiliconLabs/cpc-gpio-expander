use anyhow::{anyhow, Result};
use nom::AsBytes;
use std::sync::{mpsc, Mutex};
use thiserror::Error;

use crate::gpio::*;

const GPIO_COUNT: u8 = 16;

#[derive(Error, Debug)]
pub enum MockError {
    #[error(transparent)]
    Mock(#[from] anyhow::Error),
}

#[derive(Debug)]
struct MockGpio {
    name: String,
    value: GpioValue,
    config: GpioConfig,
    direction: GpioDirection,
}

#[derive(Debug)]
pub struct Mock {
    tx: Mutex<mpsc::Sender<Vec<u8>>>,
    rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    unique_id: u64,
    label: String,
    gpios: Mutex<Vec<MockGpio>>,
}

impl Mock {
    pub fn new(instance_name: &str) -> Result<Self> {
        let (tx, rx) = mpsc::channel();

        let unique_id = instance_name.parse().unwrap();

        let label = format!("mock-{}-label", unique_id);

        let mut gpios = vec![];

        for i in 0..GPIO_COUNT {
            let gpio = MockGpio {
                name: format!("mock-{}-gpio-{}", unique_id, i),
                value: GpioValue::Low,
                config: GpioConfig::BiasDisable,
                direction: GpioDirection::Disabled,
            };

            gpios.push(gpio);
        }

        Ok(Self {
            tx: Mutex::new(tx),
            rx: Mutex::new(rx),
            unique_id,
            label,
            gpios: Mutex::new(gpios),
        })
    }
}

impl Gpio for Mock {
    fn write(&self, data: &[u8]) -> Result<(), Error> {
        self.tx
            .lock()
            .map_err(|err| UnrecoverableError::Anyhow(anyhow!("{}", err)))?
            .send(data.to_vec())
            .map_err(|err| UnrecoverableError::Interface(anyhow!("{}", err).into()))?;

        Ok(())
    }

    fn read(&self) -> Result<Vec<u8>, Error> {
        let data = self
            .rx
            .lock()
            .map_err(|err| UnrecoverableError::Anyhow(anyhow!("{}", err)))?
            .recv()
            .map_err(|err| UnrecoverableError::Interface(anyhow!("{}", err).into()))?;

        let mut packet = vec![];

        let (remaining, header) = deserialize_header(&data).unwrap();

        match header.cmd {
            packet::HostCmd::GetVersion => {
                packet.push(packet::SecondaryCmd::VersionIs as u8);
                packet.push(std::mem::size_of::<utils::Version>() as u8);
                packet.push(VERSION.major);
                packet.push(VERSION.minor);
                packet.push(VERSION.patch);
            }
            packet::HostCmd::GetUniqueId => {
                let (_, host_header) = deserialize_host_header(remaining).unwrap();
                let len = std::mem::size_of_val(&host_header) as u8
                    + std::mem::size_of_val(&self.unique_id) as u8;

                let mut uid = bincode::serialize(&self.unique_id).unwrap();

                packet.push(packet::SecondaryCmd::UniqueIdIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.append(&mut uid);
            }
            packet::HostCmd::GetChipLabel => {
                let (_, host_header) = deserialize_host_header(remaining).unwrap();
                let mut label = std::ffi::CString::new(&*self.label)
                    .unwrap()
                    .as_bytes_with_nul()
                    .as_bytes()
                    .to_vec();

                let len = std::mem::size_of_val(&host_header) as u8 + label.len() as u8;

                packet.push(packet::SecondaryCmd::ChipLabelIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.append(&mut label);
            }
            packet::HostCmd::GetGpioCount => {
                let gpios = self.gpios.lock().unwrap();
                let (_, host_header) = deserialize_host_header(remaining).unwrap();
                let count = gpios.len() as u8;
                let len =
                    std::mem::size_of_val(&host_header) as u8 + std::mem::size_of_val(&count) as u8;

                packet.push(packet::SecondaryCmd::GpioCountIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.push(count);
            }
            packet::HostCmd::GetGpioName => {
                let gpios = self.gpios.lock().unwrap();
                let (remaining, host_header) = deserialize_host_header(remaining).unwrap();
                let (_, pin) = deserialize_pin(remaining).unwrap();

                let mut name = std::ffi::CString::new(&*gpios[pin as usize].name)
                    .unwrap()
                    .as_bytes_with_nul()
                    .as_bytes()
                    .to_vec();

                let len = std::mem::size_of_val(&host_header) as u8 + name.len() as u8;

                packet.push(packet::SecondaryCmd::GpioNameIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.append(&mut name);
            }
            packet::HostCmd::GetGpioValue => {
                let gpios = self.gpios.lock().unwrap();
                let (remaining, host_header) = deserialize_host_header(remaining).unwrap();
                let (_, pin) = deserialize_pin(remaining).unwrap();
                let value = gpios[pin as usize].value;
                let len = std::mem::size_of_val(&host_header) as u8
                    + std::mem::size_of_val(&gpios[pin as usize].value) as u8;

                packet.push(packet::SecondaryCmd::GpioValueIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.push(value as u8);
            }
            packet::HostCmd::SetGpioValue => {
                let mut gpios = self.gpios.lock().unwrap();
                let (remaining, host_header) = deserialize_host_header(remaining).unwrap();
                let (remaining, pin) = deserialize_pin(remaining).unwrap();
                let (_, value) = deserialize_value(remaining).unwrap();
                let len =
                    std::mem::size_of_val(&host_header) as u8 + std::mem::size_of::<Status>() as u8;

                gpios[pin as usize].value = value;

                packet.push(packet::SecondaryCmd::StatusIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.push(packet::Status::Ok as u8);
            }
            packet::HostCmd::SetGpioConfig => {
                let mut gpios = self.gpios.lock().unwrap();
                let (remaining, host_header) = deserialize_host_header(remaining).unwrap();
                let (remaining, pin) = deserialize_pin(remaining).unwrap();
                let (_, config) = deserialize_config(remaining).unwrap();
                let len =
                    std::mem::size_of_val(&host_header) as u8 + std::mem::size_of::<Status>() as u8;

                gpios[pin as usize].config = config;

                packet.push(packet::SecondaryCmd::StatusIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.push(packet::Status::Ok as u8);
            }
            packet::HostCmd::SetGpioDirection => {
                let mut gpios = self.gpios.lock().unwrap();
                let (remaining, host_header) = deserialize_host_header(remaining).unwrap();
                let (remaining, pin) = deserialize_pin(remaining).unwrap();
                let (_, direction) = deserialize_direction(remaining).unwrap();
                let len =
                    std::mem::size_of_val(&host_header) as u8 + std::mem::size_of::<Status>() as u8;

                match direction {
                    GpioDirection::Output => (),
                    GpioDirection::Input => (),
                    GpioDirection::Disabled => gpios[pin as usize].value = packet::GpioValue::Low,
                }

                gpios[pin as usize].direction = direction;

                packet.push(packet::SecondaryCmd::StatusIs as u8);
                packet.push(len);
                packet.push(host_header.seq);

                packet.push(packet::Status::Ok as u8);
            }
            packet::HostCmd::UnknownCmd => panic!(),
        }

        Ok(packet)
    }
}

fn deserialize_cmd(input: &[u8]) -> nom::IResult<&[u8], packet::HostCmd> {
    let (remaining, cmd) = nom::number::complete::u8(input)?;
    let cmd = packet::HostCmd::try_from(cmd).unwrap_or(packet::HostCmd::UnknownCmd);
    Ok((remaining, cmd))
}

fn deserialize_header(input: &[u8]) -> nom::IResult<&[u8], packet::Header<packet::HostCmd>> {
    let (remaining, cmd) = deserialize_cmd(input)?;
    let (remaining, len) = nom::number::complete::u8(remaining)?;
    Ok((remaining, packet::Header::new(cmd, len)))
}

fn deserialize_host_header(input: &[u8]) -> nom::IResult<&[u8], packet::HostHeader> {
    let (remaining, seq) = nom::number::complete::u8(input)?;
    Ok((remaining, packet::HostHeader { seq }))
}

fn deserialize_pin(input: &[u8]) -> nom::IResult<&[u8], u8> {
    let (remaining, pin) = nom::number::complete::u8(input)?;
    Ok((remaining, pin))
}

fn deserialize_value(input: &[u8]) -> nom::IResult<&[u8], GpioValue> {
    let (remaining, value) = nom::number::complete::u8(input)?;
    Ok((remaining, GpioValue::try_from(value).unwrap()))
}

fn deserialize_direction(input: &[u8]) -> nom::IResult<&[u8], GpioDirection> {
    let (remaining, direction) = nom::number::complete::u8(input)?;
    Ok((remaining, GpioDirection::try_from(direction).unwrap()))
}

fn deserialize_config(input: &[u8]) -> nom::IResult<&[u8], GpioConfig> {
    let (remaining, config) = nom::number::complete::u8(input)?;
    Ok((remaining, GpioConfig::try_from(config).unwrap()))
}
