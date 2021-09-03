use std::{
    fmt::Debug,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use egg_mode::{
    auth::{self, KeyPair, Token},
    media::{media_types, upload_media},
    tweet::DraftTweet,
};
use serde::Deserialize;
use tokio::sync::{broadcast, watch};
use tracing::{debug, info, trace, warn};

use crate::measure::Measurement;

#[derive(Deserialize, Clone)]
pub(crate) struct Config {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_secret: String,
}

impl Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "twitter::Config{{ CREDENTIALS REDACTED }}")
    }
}

pub(crate) struct Twitter {
    token: Token,
    plot_path: Option<PathBuf>,
    rx: broadcast::Receiver<Measurement>,
    quit: watch::Receiver<bool>,
}

impl Twitter {
    #[tracing::instrument(skip(rx, quit))]
    pub(crate) async fn new<P: AsRef<Path> + Debug>(
        config: &Config,
        plot_path: Option<P>,
        rx: broadcast::Receiver<Measurement>,
        quit: watch::Receiver<bool>,
    ) -> Result<Self> {
        trace!("Creating new '{}'...", std::any::type_name::<Self>());

        let token = Token::Access {
            consumer: KeyPair::new(config.consumer_key.clone(), config.consumer_secret.clone()),
            access: KeyPair::new(config.access_token.clone(), config.access_secret.clone()),
        };
        let resp = auth::verify_tokens(&token)
            .await
            .with_context(|| "failed to verify the given Twitter tokens")?;
        info!(
            "Credentials verified for {} (@{})",
            resp.response.name, resp.response.screen_name
        );
        debug!("{:?}", resp.response);
        debug!(
            "Current rate limiting information: {:?}",
            resp.rate_limit_status
        );

        Ok(Self {
            token,
            plot_path: plot_path.map(|p| p.as_ref().to_owned()),
            rx,
            quit,
        })
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn run(mut self) {
        let mut last_tweet_id: Option<u64> = None;
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
                            last_tweet_id = Self::tweet(
                                measurements,
                                &self.token,
                                last_tweet_id,
                                self.plot_path.as_ref(),
                            )
                            .await
                        },
                        Err(e) => {
                            warn!("Failed to receive from the measurements channel: {}", e);
                        },
                    };
                },
            }
        }
    }

    #[tracing::instrument(skip(token, _plot_path))]
    async fn tweet<P: AsRef<Path> + Debug>(
        measurement: Measurement,
        token: &Token,
        mut last_tweet_id: Option<u64>,
        _plot_path: Option<P>,
    ) -> Option<u64> {
        // Crate a new draft tweet
        let tweet_text = format!(
            "Latest Measurement:\n⛖ Ping Latency: {:.3}ms\n⬇ Download Bandwidth: {:.3} Mbps\n⬆ Upload Bandwidth: {:.3} Mbps\n",
            measurement.ping_latency, measurement.download_speed, measurement.upload_speed
        );
        let mut draft = DraftTweet::new(tweet_text);
        if let Some(last_tweet_id) = last_tweet_id {
            draft = draft.in_reply_to(last_tweet_id);
        }

        // If Cargo feature "plot" is enabled and a path has been made available (indicating that
        // there is a Database with a Plotter), attach the plot as a PNG image to the draft tweet.
        // In case of failure, abort returning the previous tweet ID (or None if there is none).
        #[cfg(feature = "plot")]
        if let Some(plot_path) = _plot_path {
            if let Err(err) = Self::attach_plot_image(&mut draft, token, plot_path).await {
                warn!("Failed to attach plot image: {}", err);
            }
        }

        // Attempt to post the new tweet
        match draft.send(token).await {
            Err(err) => warn!("Failed to send tweet: {}", err),
            Ok(resp) => {
                trace!("Draft tweet has been successfully sent: {:?}", resp);
                let tweet = resp.response;
                debug!(
                    "Replacing {:?} with {:?} as last_tweet_id",
                    last_tweet_id, tweet.id
                );
                let _ = last_tweet_id.replace(tweet.id);
            }
        };
        last_tweet_id
    }

    #[tracing::instrument]
    async fn attach_plot_image<P: AsRef<Path> + Debug>(
        draft: &mut DraftTweet,
        token: &Token,
        plot_path: P,
    ) -> Result<()> {
        // Read the PNG image from the filesystem
        let png = tokio::fs::read(&plot_path)
            .await
            .with_context(|| "failed to read the latest PNG plot image")?;

        // Upload the PNG image
        let handle = upload_media(&png, &media_types::image_png(), token)
            .await
            .with_context(|| "failed to upload the latest PNG plot image")?;

        // Attach the PNG image to the draft tweet
        draft.add_media(handle.id);

        Ok(())
    }
}
