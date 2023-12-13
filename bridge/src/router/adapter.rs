use anyhow::{bail, Result};

use crate::driver;
use crate::gpio;

impl TryFrom<&gpio::RecoverableError> for driver::Status {
    type Error = anyhow::Error;
    fn try_from(err: &gpio::RecoverableError) -> Result<Self, Self::Error> {
        match err {
            gpio::RecoverableError::Timeout(timeout, ms) => {
                bail!("Timeout({}: {} ms)", timeout, ms)
            }
            gpio::RecoverableError::Deserialization(_) => Ok(driver::Status::ProtocolError),
            gpio::RecoverableError::Serialization(_) => Ok(driver::Status::ProtocolError),
            gpio::RecoverableError::Packet(status) => Ok(status.into()),
        }
    }
}

impl From<&gpio::Status> for driver::Status {
    fn from(status: &gpio::Status) -> Self {
        match status {
            gpio::Status::Ok => driver::Status::Ok,
            gpio::Status::NotSupported => driver::Status::NotSupported,
            gpio::Status::InvalidPin => driver::Status::ProtocolError,
            gpio::Status::Unknown => driver::Status::Unknown,
        }
    }
}

impl From<&anyhow::Error> for driver::Status {
    fn from(err: &anyhow::Error) -> Self {
        if let Some(err) = err.downcast_ref::<gpio::RecoverableError>() {
            err.try_into().unwrap_or(driver::Status::Unknown)
        } else {
            driver::Status::Unknown
        }
    }
}

impl From<driver::GpioValue> for gpio::GpioValue {
    fn from(direction: driver::GpioValue) -> gpio::GpioValue {
        match direction {
            driver::GpioValue::Low => gpio::GpioValue::Low,
            driver::GpioValue::High => gpio::GpioValue::High,
        }
    }
}

impl From<driver::GpioDirection> for gpio::GpioDirection {
    fn from(direction: driver::GpioDirection) -> gpio::GpioDirection {
        match direction {
            driver::GpioDirection::Input => gpio::GpioDirection::Input,
            driver::GpioDirection::Output => gpio::GpioDirection::Output,
            driver::GpioDirection::Disabled => gpio::GpioDirection::Disabled,
        }
    }
}

impl From<driver::GpioConfig> for gpio::GpioConfig {
    fn from(config: driver::GpioConfig) -> gpio::GpioConfig {
        match config {
            driver::GpioConfig::BiasDisable => gpio::GpioConfig::BiasDisable,
            driver::GpioConfig::BiasPullDown => gpio::GpioConfig::BiasPullDown,
            driver::GpioConfig::BiasPullUp => gpio::GpioConfig::BiasPullUp,
            driver::GpioConfig::DriveOpenDrain => gpio::GpioConfig::DriveOpenDrain,
            driver::GpioConfig::DriveOpenSource => gpio::GpioConfig::DriveOpenSource,
            driver::GpioConfig::DrivePushPull => gpio::GpioConfig::DrivePushPull,
        }
    }
}
