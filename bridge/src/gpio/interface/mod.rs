use anyhow::Result;

use super::GpioTraits;
use crate::utils;

#[cfg(feature = "gpio_mock")]
mod mock;
#[cfg(feature = "gpio_mock")]
pub use mock::MockError as Error;

#[cfg(feature = "gpio_cpc")]
mod cpc;
#[cfg(feature = "gpio_cpc")]
pub use cpc::CpcError as Error;

pub fn new(config: &utils::Config, _trace_config: &utils::TraceConfig) -> Result<Box<GpioTraits>> {
    #[cfg(feature = "gpio_mock")]
    let interface = mock::Mock::new(&config.instance)?;

    #[cfg(feature = "gpio_cpc")]
    let interface = cpc::Cpc::new(&config.instance, _trace_config.libcpc)?;

    Ok(Box::new(interface))
}
