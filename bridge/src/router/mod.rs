use anyhow::{anyhow, bail, Result};
use mio::{unix::SourceFd, Events, Interest, Poll, Token};
use mio_signals::{Signal, Signals};

use crate::driver;
use crate::endpoint;
use crate::utils;

mod adapter;

pub const SIGNAL_TOKEN: Token = Token(0);
pub const DRIVER_TOKEN: Token = Token(1);
pub const ENDPOINT_TOKEN: Token = Token(2);

pub fn process_loop(
    signals: &mut Signals,
    driver: &mut driver::Driver,
    endpoint: &mut endpoint::Endpoint,
) -> Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(3);

    poll.registry()
        .register(signals, SIGNAL_TOKEN, Interest::READABLE)?;

    let mut driver_fd = SourceFd(&driver.fd);

    poll.registry()
        .register(&mut driver_fd, DRIVER_TOKEN, Interest::READABLE)?;

    poll.registry().register(
        &mut endpoint.exit_signal,
        ENDPOINT_TOKEN,
        Interest::READABLE,
    )?;

    loop {
        poll.poll(&mut events, None)?;
        for event in events.iter() {
            match event.token() {
                SIGNAL_TOKEN => on_signals(signals, driver, endpoint)?,
                DRIVER_TOKEN => on_driver(driver, endpoint)?,
                ENDPOINT_TOKEN => on_endpoint_token(driver, endpoint)?,
                _ => log::warn!("Unexpected event: {:?}", event),
            }
        }
    }
}

fn on_endpoint_token(driver: &mut driver::Driver, endpoint: &mut endpoint::Endpoint) -> Result<()> {
    let context = endpoint.read_exit_signal()?;
    if let Err(err) = driver.deinit(endpoint.gpio_chip.unique_id) {
        bail!(format!("{}, {}", context, err));
    } else {
        bail!(format!("{}", context));
    }
}

fn on_signals(
    signals: &mut Signals,
    driver: &mut driver::Driver,
    endpoint: &mut endpoint::Endpoint,
) -> Result<()> {
    loop {
        if let Some(signal) = signals.receive()? {
            match signal {
                Signal::Interrupt | Signal::Terminate => {
                    let context = format!("Received signal: {:?}", signal);
                    if let Err(err) = driver.deinit(endpoint.gpio_chip.unique_id) {
                        bail!(format!("{}, {}", context, err));
                    } else {
                        bail!(utils::Exit::Context(anyhow!(context)));
                    }
                }
                _ => log::warn!("Received unexpected signal: {:?}", signal),
            }
        } else {
            break;
        }
    }

    Ok(())
}

fn on_driver(driver: &mut driver::Driver, endpoint: &mut endpoint::Endpoint) -> Result<()> {
    loop {
        if let Some(packet) = driver.read()? {
            let result = match driver.parse(packet, endpoint.gpio_chip.unique_id) {
                Ok(packet) => match &packet {
                    driver::Packet::Discard => Ok(()),
                    driver::Packet::GetGpioValue(packet) => {
                        on_gpio_get_value(driver, endpoint, packet)
                    }
                    driver::Packet::SetGpioValue(packet) => {
                        on_gpio_set_value(driver, endpoint, packet)
                    }
                    driver::Packet::SetGpioConfig(packet) => {
                        on_gpio_set_config(driver, endpoint, packet)
                    }
                    driver::Packet::SetGpioDirection(packet) => {
                        on_gpio_set_direction(driver, endpoint, packet)
                    }
                    driver::Packet::Exit(packet) => {
                        bail!(utils::Exit::Context(anyhow!("{}", packet.message)));
                    }
                },
                Err(err) => Err(err),
            };

            if let Err(err) = result {
                log::warn!("Err: {}", err);
            }
        } else {
            break;
        }
    }

    Ok(())
}

fn on_gpio_get_value(
    driver: &mut driver::Driver,
    endpoint: &mut endpoint::Endpoint,
    packet: &driver::GetGpioValue,
) -> Result<()> {
    let (value, status) = match endpoint.get_gpio_value(packet.pin.try_into()?) {
        Ok(gpio_value) => match gpio_value.value {
            Ok(value) => {
                log::debug!("{:?}", packet);
                (Some(value as u32), Some(driver::Status::Ok))
            }
            Err(err) => {
                log::warn!("{:?}, Err: {}", packet, err);
                (None, (&err).try_into().ok())
            }
        },
        Err(err) => {
            log::warn!("{:?}, Err: {}", packet, err);
            (None, (&err).try_into().ok())
        }
    };

    driver.get_gpio_value_reply(endpoint.gpio_chip.unique_id, packet.pin, value, status)?;

    Ok(())
}

fn on_gpio_set_value(
    driver: &mut driver::Driver,
    endpoint: &mut endpoint::Endpoint,
    packet: &driver::SetGpioValue,
) -> Result<()> {
    let status = match endpoint.set_gpio_value(packet.pin.try_into()?, packet.value.into()) {
        Ok(_) => {
            log::debug!("{:?}", packet);
            Some(driver::Status::Ok)
        }
        Err(err) => {
            log::warn!("{:?}, Err: {}", packet, err);
            (&err).try_into().ok()
        }
    };

    driver.set_gpio_value_reply(endpoint.gpio_chip.unique_id, packet.pin, status)?;

    Ok(())
}

fn on_gpio_set_config(
    driver: &mut driver::Driver,
    endpoint: &mut endpoint::Endpoint,
    packet: &driver::SetGpioConfig,
) -> Result<()> {
    let status = match endpoint.set_gpio_config(packet.pin.try_into()?, packet.config.into()) {
        Ok(_) => {
            log::debug!("{:?}", packet);
            Some(driver::Status::Ok)
        }
        Err(err) => {
            log::warn!("{:?}, Err: {}", packet, err);
            (&err).try_into().ok()
        }
    };

    driver.set_gpio_config_reply(endpoint.gpio_chip.unique_id, packet.pin, status)?;

    Ok(())
}

fn on_gpio_set_direction(
    driver: &mut driver::Driver,
    endpoint: &mut endpoint::Endpoint,
    packet: &driver::SetGpioDirection,
) -> Result<()> {
    let status = match endpoint.set_gpio_direction(packet.pin.try_into()?, packet.direction.into())
    {
        Ok(_) => {
            log::debug!("{:?}", packet);
            Some(driver::Status::Ok)
        }
        Err(err) => {
            log::warn!("{:?}, Err: {}", packet, err);
            (&err).try_into().ok()
        }
    };

    driver.set_gpio_direction_reply(endpoint.gpio_chip.unique_id, packet.pin, status)?;

    Ok(())
}
