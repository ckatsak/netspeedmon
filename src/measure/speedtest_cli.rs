use async_trait::async_trait;
use tokio::{process::Command, time::Instant};
use tracing::{debug, error, trace, warn};

use super::{Measurement, Measurer};

#[derive(Debug, Default)]
pub struct SpeedTestCli;

#[async_trait]
impl Measurer for SpeedTestCli {
    #[tracing::instrument]
    async fn measure(&mut self, deadline: Instant) -> Measurement {
        let fork_output = Command::new("speedtest")
            .args(&["--format", "json", "--accept-gdpr"])
            .kill_on_drop(true)
            .output();

        trace!("Now blocking, waiting for execution to complete or to time out...");
        let out = match tokio::time::timeout_at(deadline, fork_output).await {
            Err(task_timeout_err) => {
                error!(
                    "Timed out waiting for the 'speedtest' binary to complete its execution: {}",
                    task_timeout_err
                );
                return Default::default();
            }
            Ok(task_result) => match task_result {
                Err(io_err) => {
                    error!(
                        "Failed to spawn the 'speedtest' binary or to retrieve its output: {}",
                        io_err
                    );
                    return Default::default();
                }
                Ok(out) => {
                    debug!(
                        "The execution of the 'speedtest' binary finished with '{}' and stdout: '{:?}'",
                        out.status,
                        std::str::from_utf8(&out.stdout)
                    );
                    out
                }
            },
        };

        // Handle errors or indications thereof
        if !out.stderr.is_empty() {
            warn!(
                "The execution of the 'speedtest' binary finished with a non-empty stderr: '{:?}'",
                std::str::from_utf8(&out.stderr)
            );
        }
        if !out.status.success() {
            warn!(
                "The execution of the 'speedtest' binary failed with code '{:?}'",
                out.status.code(),
            );
            if out.stdout.is_empty() {
                error!("The execution of the 'speedtest' binary failed with empty stdout");
                return Default::default();
            }
        }

        // Parse the non-empty stdout regardless of whether the execution succeeded or failed
        let root: serde_json::Value = match serde_json::from_slice(&out.stdout) {
            Ok(root_object) => root_object,
            Err(e) => {
                error!(
                    "Failed to deserialize the JSON output of the 'speedtest' binary: {}",
                    e
                );
                return Default::default();
            }
        };

        // If the execution failed, report it and exit
        if !out.status.success() {
            error!(
                "Binary 'speedtest' reported: '{}'",
                root["error"].to_string()
            );
            return Default::default();
        }

        // If the execution succeeded, attempt to decode the measurements
        let ping_latency = root["ping"]["latency"]
            .as_f64()
            .expect("failed to retrieve ping latency as f64");
        let download_speed = (root["download"]["bandwidth"]
            .as_u64()
            .expect("failed to retrieve download speed as u64")
            * 8) as f64
            / 1000.
            / 1000.;
        let upload_speed = (root["upload"]["bandwidth"]
            .as_u64()
            .expect("failed to retrieve upload speed as u64")
            * 8) as f64
            / 1000.
            / 1000.;

        (ping_latency, download_speed, upload_speed).into()
    }
}
