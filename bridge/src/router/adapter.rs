use anyhow::{bail, Result};

use crate::driver;
use crate::endpoint;

impl TryFrom<&endpoint::Error> for driver::Status {
    type Error = anyhow::Error;
    fn try_from(err: &endpoint::Error) -> Result<Self, Self::Error> {
        match err {
            endpoint::Error::Timeout(timeout, ms) => bail!("Timeout({}: {} ms)", timeout, ms),
            endpoint::Error::Deserialization(_) => Ok(driver::Status::ProtocolError),
            endpoint::Error::Serialization(_) => Ok(driver::Status::ProtocolError),
            endpoint::Error::Libcpc(_) => Ok(driver::Status::BrokenPipe),
            endpoint::Error::Packet(status) => Ok(status.into()),
        }
    }
}

impl From<&endpoint::Status> for driver::Status {
    fn from(status: &endpoint::Status) -> Self {
        match status {
            endpoint::Status::Ok => driver::Status::Ok,
            endpoint::Status::NotSupported => driver::Status::NotSupported,
            endpoint::Status::InvalidPin => driver::Status::ProtocolError,
            endpoint::Status::Unknown => driver::Status::Unknown,
        }
    }
}

impl From<&anyhow::Error> for driver::Status {
    fn from(err: &anyhow::Error) -> Self {
        if let Some(err) = err.downcast_ref::<endpoint::Error>() {
            err.try_into().unwrap_or(driver::Status::Unknown)
        } else {
            driver::Status::Unknown
        }
    }
}

impl From<driver::GpioValue> for endpoint::GpioValue {
    fn from(direction: driver::GpioValue) -> endpoint::GpioValue {
        match direction {
            driver::GpioValue::Low => endpoint::GpioValue::Low,
            driver::GpioValue::High => endpoint::GpioValue::High,
        }
    }
}

impl From<driver::GpioDirection> for endpoint::GpioDirection {
    fn from(direction: driver::GpioDirection) -> endpoint::GpioDirection {
        match direction {
            driver::GpioDirection::Input => endpoint::GpioDirection::Input,
            driver::GpioDirection::Output => endpoint::GpioDirection::Output,
            driver::GpioDirection::Disabled => endpoint::GpioDirection::Disabled,
        }
    }
}

impl From<driver::GpioConfig> for endpoint::GpioConfig {
    fn from(config: driver::GpioConfig) -> endpoint::GpioConfig {
        match config {
            driver::GpioConfig::BiasDisable => endpoint::GpioConfig::BiasDisable,
            driver::GpioConfig::BiasPullDown => endpoint::GpioConfig::BiasPullDown,
            driver::GpioConfig::BiasPullUp => endpoint::GpioConfig::BiasPullUp,
            driver::GpioConfig::DriveOpenDrain => endpoint::GpioConfig::DriveOpenDrain,
            driver::GpioConfig::DriveOpenSource => endpoint::GpioConfig::DriveOpenSource,
            driver::GpioConfig::DrivePushPull => endpoint::GpioConfig::DrivePushPull,
        }
    }
}
