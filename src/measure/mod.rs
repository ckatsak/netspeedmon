pub(super) mod speedtest_cli;
#[cfg(feature = "zpeters")]
pub(super) mod speedtestr;

use std::fmt::Debug;

use async_trait::async_trait;
use serde::Serialize;
use tokio::time::Instant;

#[async_trait]
pub(super) trait Measurer: Debug {
    async fn measure(&mut self, deadline: Instant) -> Measurement;
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Measurement {
    pub ping_latency: f64,
    pub download_speed: f64,
    pub upload_speed: f64,
}

impl From<(f64, f64, f64)> for Measurement {
    fn from((ping_latency, download_speed, upload_speed): (f64, f64, f64)) -> Self {
        Self {
            ping_latency,
            download_speed,
            upload_speed,
        }
    }
}
