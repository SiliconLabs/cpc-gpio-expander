use mio_signals::{Signal, Signals};

mod driver;
mod gpio;
mod router;
mod utils;

fn main() -> ! {
    let config: utils::Config = clap::Parser::parse();
    let trace_config = utils::trace(&config);

    env_logger::Builder::new()
        .filter(Some(module_path!()), trace_config.bridge)
        .format_target(false)
        .format_timestamp(Some(env_logger::TimestampPrecision::Millis))
        .init();

    log::info!(
        "[CPC GPIO Bridge v{}] [GPIO API v{}] [Driver API v{}]",
        env!("CARGO_PKG_VERSION"),
        gpio::VERSION,
        driver::VERSION
    );

    log::info!("{:?}", config);

    let run = || {
        let lock_file = std::path::Path::new(&config.lock_dir)
            .join(format!("cpc-gpio-bridge-{}.lock", config.instance));

        let _bridge_lock = utils::lock_bridge(&lock_file)?;

        let signals = Signals::new(Signal::Interrupt | Signal::Terminate | Signal::User1)?;

        let gpio = gpio::Handle::new(&config, &trace_config)?;

        let driver = driver::Handle::new(
            config.deinit,
            gpio.chip.unique_id,
            &gpio.chip.label,
            &gpio.chip.gpio_names,
        )?;

        router::process_loop(signals, driver, gpio)?;

        Ok(())
    };

    if let Err(err) = run() {
        utils::exit(err);
    }

    unreachable!();
}
