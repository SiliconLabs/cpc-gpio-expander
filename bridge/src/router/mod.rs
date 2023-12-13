use anyhow::{anyhow, bail, Result};
use mio::{Events, Interest, Poll, Token};
use mio_signals::{Signal, Signals};
use std::sync::Arc;
use std::sync::Mutex;

use crate::driver;
use crate::gpio;
use crate::utils;

mod adapter;

const SIGNAL_EXIT_TOKEN: Token = Token(0);
const GPIO_EXIT_TOKEN: Token = Token(1);
const DRIVER_EXIT_TOKEN: Token = Token(2);
const ROUTER_EXIT_TOKEN: Token = Token(3);
const DRIVER_UNLOAD_EXIT_TOKEN: Token = Token(4);

pub fn process_loop(
    mut signals: Signals,
    mut driver: driver::Handle,
    mut gpio: gpio::Handle,
) -> Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(4);

    let (mut router_exit_sender, router_exit_receiver) = mio::unix::pipe::new()?;
    let mut router_exit = utils::ThreadExit {
        receiver: Mutex::new(router_exit_receiver),
    };

    poll.registry().register(
        router_exit
            .receiver
            .get_mut()
            .map_err(|err| anyhow!("{}", err))?,
        ROUTER_EXIT_TOKEN,
        Interest::READABLE,
    )?;

    let (mut driver_unload_exit_sender, driver_unload_exit_receiver) = mio::unix::pipe::new()?;
    let mut driver_unload_exit = utils::ThreadExit {
        receiver: Mutex::new(driver_unload_exit_receiver),
    };

    poll.registry().register(
        driver_unload_exit
            .receiver
            .get_mut()
            .map_err(|err| anyhow!("{}", err))?,
        DRIVER_UNLOAD_EXIT_TOKEN,
        Interest::READABLE,
    )?;

    poll.registry()
        .register(&mut signals, SIGNAL_EXIT_TOKEN, Interest::READABLE)?;

    poll.registry().register(
        gpio.exit
            .receiver
            .get_mut()
            .map_err(|err| anyhow!("{}", err))?,
        GPIO_EXIT_TOKEN,
        Interest::READABLE,
    )?;

    poll.registry().register(
        driver
            .exit
            .receiver
            .get_mut()
            .map_err(|err| anyhow!("{}", err))?,
        DRIVER_EXIT_TOKEN,
        Interest::READABLE,
    )?;

    let gpio = Arc::new(gpio);
    let gpio_ref = gpio.clone();

    let driver = Arc::new(driver);
    let driver_ref = driver.clone();

    std::thread::Builder::new()
        .name("router".to_string())
        .spawn(move || {
            let gpio = gpio_ref;
            let driver = driver_ref;
            loop {
                let packet = match driver.read() {
                    Ok(packet) => packet,
                    Err(err) => {
                        utils::ThreadExit::notify(
                            &mut router_exit_sender,
                            &format!("Failed to read from Driver channel, Err: {}", err),
                        );
                        return;
                    }
                };

                let result = match driver.parse(packet) {
                    Ok(packet) => match &packet {
                        driver::Packet::GetGpioValue(packet) => {
                            on_gpio_get_value(&driver, &gpio, packet)
                        }
                        driver::Packet::SetGpioValue(packet) => {
                            on_gpio_set_value(&driver, &gpio, packet)
                        }
                        driver::Packet::SetGpioConfig(packet) => {
                            on_gpio_set_config(&driver, &gpio, packet)
                        }
                        driver::Packet::SetGpioDirection(packet) => {
                            on_gpio_set_direction(&driver, &gpio, packet)
                        }
                        driver::Packet::Exit(packet) => {
                            utils::ThreadExit::notify(
                                &mut driver_unload_exit_sender,
                                &format!("{}", packet.message),
                            );
                            return;
                        }
                    },
                    Err(err) => Err(err),
                };

                if let Err(err) = result {
                    utils::ThreadExit::notify(&mut router_exit_sender, &format!("{}", err));
                    return;
                }
            }
        })?;

    loop {
        poll.poll(&mut events, None)?;
        for event in events.iter() {
            match event.token() {
                SIGNAL_EXIT_TOKEN => on_signal_exit(&mut signals, &driver, &gpio)?,
                GPIO_EXIT_TOKEN => on_gpio_thread_exit(&driver, &gpio)?,
                DRIVER_EXIT_TOKEN => on_driver_thread_exit(&driver, &gpio)?,
                ROUTER_EXIT_TOKEN => on_router_thread_exit(&router_exit, &driver, &gpio)?,
                DRIVER_UNLOAD_EXIT_TOKEN => on_driver_unload_exit(&driver_unload_exit)?,
                _ => log::warn!("Unexpected event: {:?}", event),
            }
        }
    }
}

fn on_gpio_thread_exit(driver: &driver::Handle, gpio: &gpio::Handle) -> Result<()> {
    if let Err(err) = driver.deinit(gpio.chip.unique_id) {
        bail!(format!("{}, {}", gpio.exit, err));
    } else {
        bail!(format!("{}", gpio.exit));
    }
}

fn on_driver_thread_exit(driver: &driver::Handle, gpio: &gpio::Handle) -> Result<()> {
    if let Err(err) = driver.deinit(gpio.chip.unique_id) {
        bail!(format!("{}, {}", driver.exit, err));
    } else {
        bail!(format!("{}", driver.exit));
    }
}

fn on_router_thread_exit(
    exit: &utils::ThreadExit,
    driver: &driver::Handle,
    gpio: &gpio::Handle,
) -> Result<()> {
    if let Err(err) = driver.deinit(gpio.chip.unique_id) {
        bail!(format!("{}, {}", exit, err));
    } else {
        bail!(format!("{}", exit));
    }
}

fn on_driver_unload_exit(exit: &utils::ThreadExit) -> Result<()> {
    bail!(utils::ProcessExit::Context(anyhow!(format!("{}", exit))));
}

fn on_signal_exit(
    signals: &mut Signals,
    driver: &driver::Handle,
    gpio: &gpio::Handle,
) -> Result<()> {
    loop {
        if let Some(signal) = signals.receive()? {
            match signal {
                Signal::Interrupt | Signal::Terminate | Signal::User1 => {
                    let context = format!("Received signal: {:?}", signal);
                    if let Err(err) = driver.deinit(gpio.chip.unique_id) {
                        bail!(format!("{}, {}", context, err));
                    } else {
                        bail!(utils::ProcessExit::Context(anyhow!(context)));
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

fn on_gpio_get_value(
    driver: &driver::Handle,
    gpio: &gpio::Handle,
    packet: &driver::GetGpioValue,
) -> Result<()> {
    log::debug!("UID {{ {:?} }} {:?}", gpio.chip.unique_id, packet);
    let (value, status) = match gpio.get_gpio_value(packet.pin.try_into()?) {
        Ok(gpio_value) => match gpio_value.value {
            Ok(value) => (Some(value as u32), Some(driver::Status::Ok)),
            Err(err) => {
                log::warn!("{:?}, Err: {}", packet, err);
                (None, (&err).try_into().ok())
            }
        },
        Err(err) => match err {
            gpio::Error::Recoverable(err) => {
                log::warn!("{:?}, Err: {}", packet, err);
                (None, (&err).try_into().ok())
            }
            gpio::Error::Unrecoverable(err) => bail!("{}", err),
        },
    };

    driver.get_gpio_value_reply(gpio.chip.unique_id, packet.pin, value, status)?;

    Ok(())
}

fn on_gpio_set_value(
    driver: &driver::Handle,
    gpio: &gpio::Handle,
    packet: &driver::SetGpioValue,
) -> Result<()> {
    log::debug!("UID {{ {:?} }} {:?}", gpio.chip.unique_id, packet);
    let status = match gpio.set_gpio_value(packet.pin.try_into()?, packet.value.into()) {
        Ok(_) => Some(driver::Status::Ok),
        Err(err) => match err {
            gpio::Error::Recoverable(err) => {
                log::warn!("{:?}, Err: {}", packet, err);
                (&err).try_into().ok()
            }
            gpio::Error::Unrecoverable(err) => bail!("{}", err),
        },
    };

    driver.set_gpio_value_reply(gpio.chip.unique_id, packet.pin, status)?;

    Ok(())
}

fn on_gpio_set_config(
    driver: &driver::Handle,
    gpio: &gpio::Handle,
    packet: &driver::SetGpioConfig,
) -> Result<()> {
    log::debug!("UID {{ {:?} }} {:?}", gpio.chip.unique_id, packet);
    let status = match gpio.set_gpio_config(packet.pin.try_into()?, packet.config.into()) {
        Ok(_) => Some(driver::Status::Ok),
        Err(err) => match err {
            gpio::Error::Recoverable(err) => {
                log::warn!("{:?}, Err: {}", packet, err);
                (&err).try_into().ok()
            }
            gpio::Error::Unrecoverable(err) => bail!("{}", err),
        },
    };

    driver.set_gpio_config_reply(gpio.chip.unique_id, packet.pin, status)?;

    Ok(())
}

fn on_gpio_set_direction(
    driver: &driver::Handle,
    gpio: &gpio::Handle,
    packet: &driver::SetGpioDirection,
) -> Result<()> {
    log::debug!("UID {{ {:?} }} {:?}", gpio.chip.unique_id, packet);
    let status = match gpio.set_gpio_direction(packet.pin.try_into()?, packet.direction.into()) {
        Ok(_) => Some(driver::Status::Ok),
        Err(err) => match err {
            gpio::Error::Recoverable(err) => {
                log::warn!("{:?}, Err: {}", packet, err);
                (&err).try_into().ok()
            }
            gpio::Error::Unrecoverable(err) => bail!("{}", err),
        },
    };

    driver.set_gpio_direction_reply(gpio.chip.unique_id, packet.pin, status)?;

    Ok(())
}
