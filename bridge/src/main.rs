use mio_signals::{Signal, Signals};

mod driver;
mod endpoint;
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
        "[CPC GPIO Bridge v{}] [Endpoint API v{}] [Driver API v{}]",
        env!("CARGO_PKG_VERSION"),
        endpoint::VERSION,
        driver::VERSION
    );

    log::info!("{:?}", config);

    let lock_file = std::path::Path::new(&config.lock_dir)
        .join(format!("cpc-gpio-bridge-{}.lock", config.instance));

    let _bridge_lock = match utils::lock_bridge(&lock_file) {
        Ok(bridge_lock) => bridge_lock,
        Err(err) => utils::exit(err),
    };

    let mut signals = match Signals::new(Signal::Interrupt | Signal::Terminate) {
        Ok(signals) => signals,
        Err(err) => utils::exit(err.into()),
    };

    let mut endpoint = match endpoint::Endpoint::new(&config.instance, trace_config.libcpc) {
        Ok(endpoint) => endpoint,
        Err(err) => utils::exit(err),
    };

    let mut driver = match driver::Driver::new(
        config.deinit,
        endpoint.gpio_chip.unique_id,
        &endpoint.gpio_chip.chip_label,
        &endpoint.gpio_chip.gpio_names,
    ) {
        Ok(driver) => driver,
        Err(err) => utils::exit(err),
    };

    if let Err(err) = router::process_loop(&mut signals, &mut driver, &mut endpoint) {
        utils::exit(err);
    }

    unreachable!();
}
