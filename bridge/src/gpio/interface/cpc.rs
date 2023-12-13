use anyhow::{bail, Result};
use thiserror::Error;

use crate::gpio::*;

const CPC_ENDPOINT: libcpc::cpc_endpoint_id = libcpc::cpc_endpoint_id::Service(
    libcpc::sl_cpc_service_endpoint_id_t_enum::SL_CPC_ENDPOINT_GPIO,
);

const CPC_READ_FLAGS: [libcpc::cpc_endpoint_read_flags_t_enum; 1] =
    [libcpc::cpc_endpoint_read_flags_t_enum::CPC_ENDPOINT_READ_FLAG_NONE];

const CPC_WRITE_FLAGS: [libcpc::cpc_endpoint_write_flags_t_enum; 1] =
    [libcpc::cpc_endpoint_write_flags_t_enum::CPC_ENDPOINT_WRITE_FLAG_NONE];

const CPC_TX_WINDOW_SIZE: u8 = 1;

const CPC_INIT_TIMEOUT_MS: u128 = 2000;
const CPC_INIT_RETRY_INTERVAL_MS: u64 = 100;
const CPC_ENDPOINT_INIT_TIMEOUT_MS: u128 = 2000;
const CPC_ENDPOINT_INIT_RETRY_INTERVAL_MS: u64 = 100;

#[derive(Error, Debug)]
pub enum CpcError {
    #[error(transparent)]
    Cpc(#[from] libcpc::Error),
}

#[derive(Debug, Copy, Clone)]
pub struct Cpc {
    cpc_endpoint: libcpc::cpc_endpoint,
}

impl Cpc {
    pub fn new(instance_name: &str, enable_tracing: bool) -> Result<Self> {
        let now = std::time::Instant::now();
        let cpc_handle = loop {
            match libcpc::init(instance_name, enable_tracing, None) {
                Ok(cpc_handle) => {
                    log::info!("Initialized CPCd ({})", instance_name);
                    break cpc_handle;
                }
                Err(err) => {
                    if now.elapsed().as_millis() >= CPC_INIT_TIMEOUT_MS {
                        bail!("Is CPCd running? Err: {}", err);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(
                        CPC_INIT_RETRY_INTERVAL_MS,
                    ));
                }
            };
        };

        let endpoint = CPC_ENDPOINT;

        let now = std::time::Instant::now();
        let cpc_endpoint = loop {
            match cpc_handle.open_endpoint(endpoint, CPC_TX_WINDOW_SIZE) {
                Ok(cpc_endpoint) => {
                    log::info!("Initialized CPC Endpoint ({:?})", endpoint);
                    break cpc_endpoint;
                }
                Err(err) => {
                    if now.elapsed().as_millis() >= CPC_ENDPOINT_INIT_TIMEOUT_MS {
                        bail!("Failed to initialize CPC Endpoint, Err: {}", err);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(
                        CPC_ENDPOINT_INIT_RETRY_INTERVAL_MS,
                    ));
                }
            };
        };

        Ok(Self { cpc_endpoint })
    }
}

impl Gpio for Cpc {
    fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        self.cpc_endpoint
            .write(bytes, &CPC_WRITE_FLAGS)
            .map_err(|err| UnrecoverableError::Interface(err.into()))?;

        Ok(())
    }

    fn read(&self) -> Result<Vec<u8>, Error> {
        let bytes = self
            .cpc_endpoint
            .read(&CPC_READ_FLAGS)
            .map_err(|err| UnrecoverableError::Interface(err.into()))?;

        Ok(bytes)
    }
}
