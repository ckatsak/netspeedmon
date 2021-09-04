mod inmemory;
#[cfg(feature = "plot")]
pub(crate) mod plotter;

use std::fmt::Debug;

#[cfg(feature = "plot")]
use anyhow::Context;
use anyhow::{bail, Result};
use async_trait::async_trait;
use chrono::{DateTime, Local};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, error, info, trace, warn};

use crate::measure::Measurement;

use self::inmemory::InMemory;
#[cfg(feature = "plot")]
use self::plotter::Plotter;

const DEFAULT_HISTORY_SIZE: usize = 170;

/// Configuration for the `Database` actor.
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Config {
    /// A String that uniquely identifies the type of the underlying `Store` to be used by the
    /// `Database`.
    ///
    /// Currently supported kinds:
    /// - In-memory store, using `std::vec`: `"in-memory"`, `"memory"`, `"mem"` or `"default"`;
    /// - TODO: On-disk CSV file: `"csv"`;
    kind: String,
    /// Path where netspeedmon's state may be stored. This includes storage required for the
    /// Database, as well as, optionally, for the Plotter.
    path: String,
    /// The number of past measurements to store (and, optionally, plot).
    history_size: Option<usize>,
}

#[cfg(all(feature = "plot", any(feature = "http", feature = "twitter")))]
impl Config {
    pub(crate) fn path(&self) -> &str {
        self.path.as_ref()
    }
}

#[derive(Debug)]
pub(crate) struct SyncMessage {
    measurement: Measurement,
    done: oneshot::Sender<()>,
}

impl SyncMessage {
    pub(crate) fn new(measurement: Measurement, done: oneshot::Sender<()>) -> Self {
        Self { measurement, done }
    }
}

#[async_trait]
trait Store: Send + Debug {
    async fn retrieve_history(&mut self) -> Result<Vec<(DateTime<Local>, Measurement)>>;
    async fn retrieve_most_recent(&mut self) -> Result<Option<(DateTime<Local>, Measurement)>>;
    async fn store(&mut self, timestamp: DateTime<Local>, measurement: Measurement) -> Result<()>;
}

#[derive(Debug)]
pub(crate) struct Database {
    config: Config,
    store: Box<dyn Store>,
    #[cfg(feature = "plot")]
    plotter: Plotter,
    rx: mpsc::Receiver<SyncMessage>,
    quit: watch::Receiver<bool>,
}

impl Database {
    #[tracing::instrument]
    pub(crate) fn new(
        config: Config,
        rx: mpsc::Receiver<SyncMessage>,
        quit: watch::Receiver<bool>,
    ) -> Result<Self> {
        trace!("Creating new '{}'", std::any::type_name::<Self>());
        let history_size = config.history_size.unwrap_or(DEFAULT_HISTORY_SIZE);
        #[cfg(feature = "plot")]
        let plotter = Plotter::new(&config.path).with_context(|| "failed to initialize Plotter")?;
        Ok(match config.kind.to_lowercase().as_str() {
            "in-memory" | "memory" | "mem" | "default" => Self {
                config,
                store: Box::new(InMemory::new(history_size)),
                #[cfg(feature = "plot")]
                plotter,
                rx,
                quit,
            },
            "csv" => {
                bail!("only 'in-memory' store is implemented so far");
            }
            unknown => bail!("unsupported database kind: '{}'", unknown),
        })
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn run(mut self) {
        loop {
            let recv = self.rx.recv();
            tokio::pin!(recv);

            debug!("Now blocking, waiting for either a quit signal or a new measurement...");
            tokio::select! {
                _ = self.quit.changed() => {
                    info!("Received signal to gracefully shut down");
                    break;
                },
                incoming = &mut recv => {
                    match incoming {
                        Some(sync_msg) => {
                            trace!("Received a new measurement: {:?}", sync_msg.measurement);

                            // Store the incoming new measurement to the Store
                            if let Err(e) =
                                self.store.store(Local::now(), sync_msg.measurement).await
                            {
                                error!(
                                    "Failed to store the new measurement to the underlying store: {}",
                                    e
                                );
                            } else {
                                // Optionally use the Plotter to create the new plot
                                #[cfg(feature = "plot")]
                                match self.store.retrieve_history().await {
                                    Ok(history) => {
                                        trace!(
                                            "Retrieved a total of {} measurements",
                                            history.len()
                                        );
                                        if let Err(err) = self.plotter.plot(history).await {
                                            error!("Error creating the newest plot: {}", err);
                                        }
                                    },
                                    Err(e) => {
                                        error!("Failed to retrieve history from store: {}", e);
                                    },
                                };
                            }

                            // Let the Monitor know that we are done
                            if sync_msg.done.send(()).is_err() {
                                warn!("Failed to sync with Monitor");
                            }
                        },
                        None => {
                            warn!("Monitor appears to have dropped its measurement channel's end");
                            // If Monitor's sending end has been dropped intentionally, continuing
                            // with the select's loop should allow us to process the quit signal.
                        },
                    };
                },
            }
        }
    }
}
