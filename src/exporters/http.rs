use std::{
    fmt::Debug,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use serde::Deserialize;
use tokio::sync::{broadcast, oneshot, watch};
use tracing::{debug, error, info, trace, warn};
use warp::{hyper::StatusCode, Filter};

use crate::measure::Measurement;

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Config {
    bind_addr: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Http {
    bind_addr: SocketAddr,
    plot_path: Option<PathBuf>,
    period: Duration,
    rx: broadcast::Receiver<Measurement>,
    quit: watch::Receiver<bool>,
}

impl Http {
    const DEFAULT_ADDRESS: &'static str = "0.0.0.0:54242";

    #[tracing::instrument(skip(rx, quit))]
    pub(crate) fn new<P: AsRef<Path> + Debug>(
        config: &Config,
        plot_path: Option<P>,
        period: Duration,
        rx: broadcast::Receiver<Measurement>,
        quit: watch::Receiver<bool>,
    ) -> Result<Self> {
        trace!("Creating new '{}'...", std::any::type_name::<Self>());
        let bind_addr = config
            .bind_addr
            .as_ref()
            .map_or_else(|| Self::DEFAULT_ADDRESS.parse(), |addr| addr.parse())?;
        Ok(Self {
            bind_addr,
            plot_path: plot_path.map(|p| p.as_ref().to_owned()),
            period,
            rx,
            quit,
        })
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn run(mut self) {
        // Setup and spawn the HTTP server as a separate task
        let latest_measurement = Arc::new(Mutex::new(Default::default()));

        // Endpoints
        let period = Self::endpoint_period(self.period);
        let latest = Self::endpoint_latest(latest_measurement.clone());
        let plot = Self::endpoint_plot(self.plot_path);
        let routes = period.or(latest).or(plot);

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
                        warn!("Failed to signal the HTTP server task: {:?}", e);
                    } else if let Err(e) = server_handle.await {
                        warn!("Failed to wait for the HTTP server task: {}", e);
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
                                    error!("Failed to acquire latest_measurement lock: {}", e);
                                }
                            };
                        },
                        Err(e) => {
                            warn!("Failed to receive from the measurements channel: {}", e);
                        },
                    };
                },
            }
        }
    }

    // Returns a plain Duration string, formatted in a human-readable form, according to crate
    // humantime.
    fn endpoint_period(period: Duration) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
        warp::get()
            .and(warp::path("period"))
            .and(warp::path::end())
            .map(move || warp::reply::json(&humantime::format_duration(period).to_string()))
            .with(warp::reply::with::header(
                "Content-Type",
                "application/json",
            ))
            .with(warp::trace::named("period"))
            .boxed()
    }

    // On success, it returns 200 OK along with a JSON-formatted Measurement; e.g.:
    //     {
    //         "ping_latency": 0.918,
    //         "download_speed": 941.300376,
    //         "upload_speed": 941.043264
    //     }
    // On failure, it returns 500 INTERNAL SERVER ERROR.
    fn endpoint_latest(
        latest_measurement: Arc<Mutex<Measurement>>,
    ) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
        warp::get()
            .and(warp::path("latest"))
            .and(warp::path::end())
            .map(move || match latest_measurement.lock() {
                Ok(latest_measurement) => warp::reply::with_status(
                    warp::reply::json(&*latest_measurement),
                    StatusCode::OK,
                ),
                Err(e) => {
                    error!("Failed to acquire lock for latest measurement: {}", e);
                    warp::reply::with_status(
                        warp::reply::json(&format!("Internal synchronization error: {}", e)),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                }
            })
            .with(warp::reply::with::header(
                "Content-Type",
                "application/json",
            ))
            .with(warp::trace::named("/latest"))
            .boxed()
    }

    // If the `plot` Cargo feature is enabled, this endpoint returns a plot image, either PNG (if
    // the `twitter` Cargo feature is enabled) or SVG (if the `twitter` Cargo feature is not
    // enabled).
    // If the `plot` Cargo feature is not enabled, it returns 404 and an error message as a plain
    // String.
    fn endpoint_plot(
        _plot_path: Option<PathBuf>,
    ) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
        let ret = warp::get().and(warp::path("plot")).and(warp::path::end());
        {
            #[cfg(feature = "plot")]
            {
                let ret = ret.and(warp::fs::file(_plot_path.expect("Http.plot_path is None")));
                #[cfg(feature = "twitter")]
                {
                    // If the "twitter" Cargo feature is enabled, plots are PNG, which are
                    // supported by the Twitter API.
                    ret.with(warp::reply::with::header("Content-Type", "image/png"))
                }
                #[cfg(not(feature = "twitter"))]
                {
                    // If the "twitter" Cargo feature is disabled, plots are SVG, which are better.
                    ret.with(warp::reply::with::header("Content-Type", "image/svg+xml"))
                }
            }
            #[cfg(not(feature = "plot"))]
            {
                ret.map(move || {
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
        .with(warp::trace::named("/plot"))
        .boxed()
    }
}
