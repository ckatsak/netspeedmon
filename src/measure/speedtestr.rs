use anyhow::anyhow;
use async_trait::async_trait;
use speedtestr::server;
use tokio::time::Instant;
use tracing::{error, trace};

use super::{Measurement, Measurer};

#[derive(Debug, Default)]
pub struct SpeedTestR;

impl SpeedTestR {
    const NUM_BEST_SERVER: &'static str = "5";
    const NUM_PINGS: u128 = 5;
    const NUM_DOWNLOAD_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB
    const NUM_UPLOAD_BYTES: u64 = 5 * 1024 * 1024; // 5 MiB
}

#[async_trait]
impl Measurer for SpeedTestR {
    #[tracing::instrument]
    async fn measure(&mut self, deadline: Instant) -> Measurement {
        //
        // First, find the best server to measure against
        //
        let best_server = match tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(|| {
                server::best_server(Self::NUM_BEST_SERVER).map_err(|e| {
                    anyhow!("failed to find the best server to measure against: {}", e)
                })
            }),
        )
        .await
        {
            Err(task_timeout_err) => {
                error!(
                    "The blocking task for 'speedtestr::server::best_server' timed out: {}",
                    task_timeout_err
                );
                return Default::default(); // no time left to measure ping, download & upload
            }
            Ok(task_result) => match task_result {
                Err(join_err) => {
                    error!(
                        "Failed to join the blocking task for 'speedtestr::server::best_server': {}",
                        join_err
                    );
                    return Default::default();
                }
                Ok(best_server) => match best_server {
                    Err(e) => {
                        error!("Failed to find the best server to measure against: {}", e);
                        return Default::default();
                    }
                    Ok(server) => {
                        trace!("The best server is found to be: '{:#?}'", server);
                        server
                    }
                },
            },
        };

        //
        // Now, measure the ping latency
        //
        let best_server_id = best_server.id.clone();
        let ping_latency = match tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || {
                match server::ping_server(best_server_id.as_str(), Self::NUM_PINGS) {
                    Ok(ping_latency) => ping_latency as f64,
                    Err(e) => {
                        error!("Failed to ping server: {}", e);
                        0.
                    }
                }
            }),
        )
        .await
        {
            Err(task_timeout_err) => {
                error!(
                    "The blocking task for 'speedtestr::server::ping_server' timed out: {}",
                    task_timeout_err
                );
                return Default::default(); // no time left to measure download & upload
            },
            Ok(task_result) => task_result.map_or_else(
                |join_err| {
                    error!(
                        "Failed to join the blocking task for 'speedtestr::server::ping_server': {}",
                        join_err
                    );
                    0.
                },
                |ping_latency| ping_latency,
            ),
        };

        //
        // Then, measure the download bandwidth
        //
        let best_server_id = best_server.id.clone();
        let download_speed = match tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || {
                match server::download(
                    best_server_id.as_str(),
                    Self::NUM_DOWNLOAD_BYTES.to_string().as_str(),
                ) {
                    Ok(download_speed) => download_speed,
                    Err(e) => {
                        error!("Failed to measure download speed: {}", e);
                        0.
                    }
                }
            }),
        )
        .await
        {
            Err(task_timeout_err) => {
                error!(
                    "The blocking task for 'speedtestr::server::download' timed out: {}",
                    task_timeout_err
                );
                return (ping_latency, 0., 0.).into(); // no time left to measure upload
            }
            Ok(task_result) => task_result.map_or_else(
                |join_err| {
                    error!(
                        "Failed to join the blocking task for 'speedtestr::server::download': {}",
                        join_err
                    );
                    0.
                },
                |download_speed| download_speed,
            ),
        };

        //
        // Finally, measure the upload bandwidth
        //
        let upload_speed = match tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || {
                match server::upload(
                    best_server.id.as_str(),
                    Self::NUM_UPLOAD_BYTES.to_string().as_str(),
                ) {
                    Ok(upload_speed) => upload_speed,
                    Err(e) => {
                        error!("Failed to measure upload speed: {}", e);
                        0.
                    }
                }
            }),
        )
        .await
        {
            Err(task_timeout_err) => {
                error!(
                    "The blocking task for 'speedtestr::server::upload' timed out: {}",
                    task_timeout_err
                );
                0. // exiting right after this anyway
            }
            Ok(task_result) => task_result.map_or_else(
                |join_err| {
                    error!(
                        "Failed to join the blocking task for 'speedtestr::server::upload': {}",
                        join_err
                    );
                    0.
                },
                |upload_speed| upload_speed,
            ),
        };

        (ping_latency, download_speed, upload_speed).into()
    }
}
