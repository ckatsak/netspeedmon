use tokio::{
    io::{self, AsyncWriteExt},
    sync::{broadcast, watch},
};
use tracing::{debug, info, trace, warn};

use crate::measure::Measurement;

pub(crate) struct StdOut {
    rx: broadcast::Receiver<Measurement>,
    quit: watch::Receiver<bool>,
}

impl StdOut {
    #[tracing::instrument(skip(rx, quit))]
    pub(crate) fn new(rx: broadcast::Receiver<Measurement>, quit: watch::Receiver<bool>) -> Self {
        trace!("Creating new '{}'", std::any::type_name::<Self>());
        Self { rx, quit }
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
                result = &mut recv => {
                    match result {
                        Ok(measurements) => {
                            Self::report(measurements).await;
                        },
                        Err(e) => {
                            warn!("Failed to receive from the measurements channel: {}", e);
                        },
                    }
                },
            }
        }
    }

    #[tracing::instrument]
    async fn report(measurement: Measurement) {
        let msg = format!(
            "Ping latency: {}ms; Download speed: {:.3}Mbps; Upload speed: {:.3}Mbps\n",
            measurement.ping_latency, measurement.download_speed, measurement.upload_speed
        );

        trace!("About to write to stdout and then flush it");
        let mut stdout = io::stdout();
        if let Err(e) = stdout.write_all(msg.as_bytes()).await {
            warn!("Failed to write to stdout: {}", e);
        }
        if let Err(e) = stdout.flush().await {
            warn!("Failed to flush stdout: {}", e);
        }
    }
}
