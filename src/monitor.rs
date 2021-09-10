#[cfg(any(feature = "http", feature = "twitter"))]
use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
    time::{self, Instant, Interval},
};
use tracing::{debug, error, info, trace};

#[cfg(all(feature = "plot", any(feature = "http", feature = "twitter")))]
use crate::exporters::database::plotter::PLOT_FILE_NAME;
#[cfg(feature = "http")]
use crate::exporters::http::Http;
#[cfg(feature = "twitter")]
use crate::exporters::twitter::Twitter;
use crate::{
    config::Config,
    exporters::{
        database::{self, Database},
        stdout::StdOut,
    },
    measure::{Measurement, Measurer},
};

pub(crate) struct Monitor {
    //config: Config,
    /// An implementation of a `Measurer`, which provides `Monitor` with `Measurement`s to
    /// propagate them to other actors (e.g., the Database, the exporters).
    measurer: Box<dyn Measurer>,
    /// Sending end of a `mpsc` channel to allow Monitor to broadcast new measurements to the
    /// Database task.
    db_tx: mpsc::Sender<database::SyncMessage>,
    /// Sending end of a `broadcast` channel to allow Monitor to broadcast new measurements to
    /// exporter tasks.
    exp_tx: broadcast::Sender<Measurement>,
    /// Sending end of the `watch` (spmc) channel to signal other actors (i.e., exporters and
    /// database) to gracefully terminate.
    quit: watch::Sender<bool>,
    /// Receiving end of a `mpsc` channel to be notified by the signal handling task to gracefully
    /// terminate upon signal retrieval (only for SIGINT, SIGTERM and SIGQUIT, for now).
    sqrx: mpsc::Receiver<()>,
    /// Ticks on configured periods of time to initiate new rounds of measuring and exporting.
    ticker: Interval,
    /// The `JoinHandle` for the signal handling task.
    sighandler_handle: JoinHandle<()>,
    /// The `JoinHandle`s for all other actors (apart from the signal handling task).
    exporter_handles: Vec<JoinHandle<()>>,
}

impl Monitor {
    const MEASUREMENTS_CHANNEL_CAPACITY: usize = 1024;

    #[tracing::instrument(skip(config))]
    pub(crate) async fn new(config: Config, measurer: Box<dyn Measurer>) -> Result<Self> {
        let ticker = time::interval(config.period);
        let (db_tx, exp_tx, quit, exporter_handles) = Self::spawn_exporters(&config).await?;
        let (sighandler_handle, sqrx) = Self::install_signal_handlers().await?;
        Ok(Self {
            //config,
            measurer,
            db_tx,
            exp_tx,
            quit,
            sqrx,
            ticker,
            sighandler_handle,
            exporter_handles,
        })
    }

    #[tracing::instrument(skip(self))]
    async fn shutdown(self) -> Result<()> {
        debug!(
            "Signalling all {} actors to quit...",
            self.quit.receiver_count()
        );
        self.quit.send(true)?;
        self.sighandler_handle.await?;
        self.quit.closed().await;
        futures::future::join_all(self.exporter_handles).await;
        debug!("All actors appear to have exited");
        Ok(())
    }

    #[tracing::instrument(skip(config))]
    async fn spawn_exporters(
        config: &Config,
    ) -> Result<(
        mpsc::Sender<database::SyncMessage>,
        broadcast::Sender<Measurement>,
        watch::Sender<bool>,
        Vec<JoinHandle<()>>,
    )> {
        let mut exporter_handles = vec![];

        // A watch channel to signal tasks when to quit.
        let (quit_tx, _) = watch::channel(false);
        // A broadcast channel to broadcast measurements to exporters.
        let (exp_tx, _) = broadcast::channel(Self::MEASUREMENTS_CHANNEL_CAPACITY);
        // A mpsc channel to broadcast measurements to the Database.
        let (db_tx, db_rx) = mpsc::channel(1);

        // NOTE: Now that the Database works synchronously with respect to the Monitor, it does not
        // *have* to be modeled as an actor. FIXME?
        if let Some(ref dc) = config.db_config {
            debug!("Initializing Database exporter...");
            let db = Database::new(dc.clone(), db_rx, quit_tx.subscribe())
                .with_context(|| "failed to initialize Database exporter")?;
            exporter_handles.push(tokio::spawn(async move { db.run().await }));
        }

        if config.stdout {
            debug!("Initializing Standard Output exporter...");
            let rx = exp_tx.subscribe();
            let quit = quit_tx.subscribe();
            exporter_handles.push(tokio::spawn(
                async move { StdOut::new(rx, quit).run().await },
            ));
        }

        #[cfg(any(feature = "http", feature = "twitter"))]
        let plot_path: Option<PathBuf> = config.db_config.as_ref().map(|_c| {
            #[cfg(feature = "plot")]
            {
                PathBuf::from(_c.path()).join(PLOT_FILE_NAME)
            }
            #[cfg(not(feature = "plot"))]
            {
                PathBuf::new()
            }
        });

        #[cfg(feature = "http")]
        if let Some(ref hc) = config.http_config {
            debug!("Initializing HTTP exporter...");
            let http = Http::new(
                hc,
                plot_path.as_ref(),
                config.period,
                exp_tx.subscribe(),
                quit_tx.subscribe(),
            )
            .with_context(|| "failed to initialize HTTP exporter")?;
            exporter_handles.push(tokio::spawn(async move { http.run().await }));
        }

        #[cfg(feature = "twitter")]
        if let Some(ref tc) = config.twitter_config {
            debug!("Initializing Twitter exporter...");
            let twitter = Twitter::new(
                tc,
                plot_path.as_ref(),
                exp_tx.subscribe(),
                quit_tx.subscribe(),
            )
            .await
            .with_context(|| "failed to initialize Twitter exporter")?;
            exporter_handles.push(tokio::spawn(async move { twitter.run().await }));
        }

        Ok((db_tx, exp_tx, quit_tx, exporter_handles))
    }

    #[tracing::instrument]
    async fn install_signal_handlers() -> Result<(JoinHandle<()>, mpsc::Receiver<()>)> {
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigquit = signal(SignalKind::quit())?;
        let (sqtx, sqrx) = mpsc::channel(1);
        let signal_handler = tokio::spawn(async move {
            let (sigint, sigterm, sigquit) = (sigint.recv(), sigterm.recv(), sigquit.recv());
            tokio::select! {
                _ = sigint => {
                    info!("Received a SIGINT");
                },
                _ = sigterm => {
                    info!("Received a SIGTERM");
                },
                _ = sigquit => {
                    info!("Received a SIGQUIT");
                },
            }
            info!("Beginning graceful termination...");
            sqtx.send(())
                .await
                .expect("failed to signal Monitor to begin graceful termination through sqtx");
        });
        Ok((signal_handler, sqrx))
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn run(mut self) -> Result<()> {
        loop {
            tokio::select! {
                _ = self.sqrx.recv() => {
                    return self.shutdown().await
                }
                now = self.ticker.tick() => {
                    debug!("Tick!");
                    self.measure_and_export(now).await;
                }
            }
        }
    }

    #[tracing::instrument(skip(self, start))]
    async fn measure_and_export(&mut self, start: Instant) {
        let deadline = start + self.ticker.period();

        // Acquire new measurements from the Measurer
        let latest_measurement = self.measurer.measure(deadline).await;

        // First, inform (synchronously) the Database (which may optionally include the Plotter)
        trace!("Sending the newest measurement to Database, synchronously");
        let (sync_tx, mut sync_rx) = oneshot::channel();
        if let Err(e) = self
            .db_tx
            .send_timeout(
                database::SyncMessage::new(latest_measurement, sync_tx),
                deadline.saturating_duration_since(Instant::now()),
            )
            .await
        {
            error!("Failed to send measurement to Database: {}", e);
            sync_rx.close();
        } else if let Err(e) = sync_rx.await {
            error!("Failed waiting to sync with Database: {}", e);
        }

        // Then, inform (asynchronously) all other exporters
        debug!(
            "Number of active exporters-receivers: {}",
            self.exp_tx.receiver_count()
        );
        match self.exp_tx.send(latest_measurement) {
            Ok(num_recvr) => trace!("Broadcasted measurement to {} exporters", num_recvr),
            Err(e) => error!("Failed to broadcast measurement to exporters: {}", e),
        }
    }
}
