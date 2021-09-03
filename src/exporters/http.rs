use std::{
    fmt::Debug,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::Result;
use serde::Deserialize;
use tokio::sync::{broadcast, oneshot, watch};
use tracing::{debug, error, info, trace, warn};
use warp::Filter;

use crate::measure::Measurement;

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Config {
    bind_addr: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Http {
    bind_addr: SocketAddr,
    plot_path: Option<PathBuf>,
    rx: broadcast::Receiver<Measurement>,
    quit: watch::Receiver<bool>,
}

impl Http {
    const DEFAULT_ADDRESS: &'static str = "0.0.0.0:54242";

    #[tracing::instrument(skip(rx, quit))]
    pub(crate) fn new<P: AsRef<Path> + Debug>(
        config: &Config,
        plot_path: Option<P>,
        rx: broadcast::Receiver<Measurement>,
        quit: watch::Receiver<bool>,
    ) -> Result<Self> {
        trace!("Creating new '{}'", std::any::type_name::<Self>());
        let bind_addr = config
            .bind_addr
            .as_ref()
            .map_or_else(|| Self::DEFAULT_ADDRESS.parse(), |addr| addr.parse())?;
        Ok(Self {
            bind_addr,
            plot_path: plot_path.map(|p| p.as_ref().to_owned()),
            rx,
            quit,
        })
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn run(mut self) {
        // Setup and spawn the HTTP server as a separate task
        let latest_measurement = Arc::new(Mutex::new(Default::default()));
        let lm = latest_measurement.clone();

        // Endpoint '/latest'
        let latest = warp::get()
            .and(warp::path("latest"))
            .and(warp::path::end())
            .map(move || match lm.lock() {
                Ok(latest_measurement) => warp::reply::json(&*latest_measurement),
                Err(e) => {
                    error!("failed to acquire lock for latest measurement: {}", e);
                    warp::reply::json(&format!("Internal synchronization error: {}", e))
                }
            })
            .with(warp::reply::with::header(
                "Content-Type",
                "application/json",
            ))
            .with(warp::trace::named("/latest"));

        // Endpoint '/plot'
        let plot = warp::get().and(warp::path("plot")).and(warp::path::end());
        let plot = {
            #[cfg(feature = "plot")]
            {
                let plot = plot.and(warp::fs::file(self.plot_path.expect("plot_path is None!")));
                #[cfg(feature = "twitter")]
                {
                    // If the "twitter" Cargo feature is enabled, plots are PNG, which are
                    // supported by the Twitter API.
                    plot.with(warp::reply::with::header("Content-Type", "image/png"))
                }
                #[cfg(not(feature = "twitter"))]
                {
                    // If the "twitter" Cargo feature is disabled, plots are SVG, which are better.
                    plot.with(warp::reply::with::header("Content-Type", "image/svg+xml"))
                }
            }
            #[cfg(not(feature = "plot"))]
            {
                plot.map(move || {
                    let body = "The Cargo feature 'plot' MUST be enabled to serve on '/plot'";
                    warp::reply::with_status(
                        warp::reply::json(&body.to_string()),
                        warp::http::StatusCode::NOT_FOUND,
                    )
                })
                .with(warp::reply::with::header(
                    "Content-Type",
                    "application/json",
                ))
            }
        }
        .with(warp::trace::named("/plot"));

        // Combine all endpoints
        let routes = latest.or(plot);

        // We are using a `oneshot` channel to notify the server to gracefully terminate upon
        // receival of a quit signal from the `watch` channel by the Monitor.
        let (sqtx, sqrx) = oneshot::channel();
        let (addr, server) =
            warp::serve(routes).bind_with_graceful_shutdown(self.bind_addr, async {
                sqrx.await.ok();
            });
        debug!("Binding to {} and serving...", addr);
        let server_handle = tokio::spawn(server);

        // Block waiting for either a quit signal or the latest measurement
        loop {
            let recv = self.rx.recv();
            tokio::pin!(recv);

            debug!("Now blocking, waiting for either a quit signal or a new measurement...");
            tokio::select! {
                _ = self.quit.changed() => {
                    info!("Received signal to gracefully shut down");
                    if let Err(e) = sqtx.send(()) {
                        warn!("failed to signal the HTTP server task: {:?}", e);
                    } else if let Err(e) = server_handle.await {
                        warn!("failed to wait for the HTTP server task: {}", e);
                    } else {
                        info!("HTTP server task has been successfully shut down");
                    }
                    break;
                },
                result = &mut recv => {
                    match result {
                        Ok(measurements) => {
                            trace!("Serving new measurements");
                            match latest_measurement.lock() {
                                Ok(ref mut lm) => {
                                    **lm = measurements;
                                }
                                Err(e) => {
                                    error!("failed to acquire latest_measurement lock: {}", e);
                                }
                            };
                        },
                        Err(e) => {
                            warn!("failed to receive from the measurements channel: {}", e);
                        },
                    };
                },
            }
        }
    }
}
