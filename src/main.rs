mod config;
mod exporters;
mod measure;
mod monitor;

use std::io;

use anyhow::{bail, Result};
use tracing_subscriber::{filter::LevelFilter, fmt::format::FmtSpan, EnvFilter};

#[cfg(feature = "zpeters")]
use crate::measure::speedtestr::SpeedTestR;
use crate::{
    config::Config,
    measure::{speedtest_cli::SpeedTestCli, Measurer},
    monitor::Monitor,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        // Let the RUST_LOG environment variable decide the logging level, having WARN as default.
        // Try `RUST_LOG=netspeedmon=trace` to log details on the execution of this crate.
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(LevelFilter::WARN.into())
                // Uncommenting the following overrides the level for the specific module:
                //.add_directive("netspeedmon=info".parse()?),
        )
        .with_thread_ids(true)
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let config = Config::parse()?;
    let measurer = initialize_measurer(&config)?;
    Monitor::new(config, measurer).await?.run().await
}

#[tracing::instrument(skip(config))]
fn initialize_measurer(config: &Config) -> Result<Box<dyn Measurer>> {
    config
        .measurer
        .as_deref()
        .map_or(
            Ok(Box::new(SpeedTestCli::default())),
            |m| match m.to_lowercase().as_str() {
                "ookla" | "default" => Ok(Box::new(SpeedTestCli::default())),
                "zpeters/speedtestr" | "zpeters" | "speedtestr" => {
                    #[cfg(feature = "zpeters")]
                    return Ok(Box::new(SpeedTestR::default()));
                    #[cfg(not(feature = "zpeters"))]
                    bail!("The Cargo feature 'zpeters' MUST be enabled to use the 'SpeedTestR' Measurer");
                }
                m => bail!("Unknown measurer '{}'", m),
            },
        )
}
