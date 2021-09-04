use std::{
    fmt::Debug,
    fs::{metadata, remove_file},
    io::ErrorKind,
    ops::Range,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Local};
#[cfg(feature = "twitter")]
use plotters::prelude::BitMapBackend;
#[cfg(not(feature = "twitter"))]
use plotters::prelude::SVGBackend;
use plotters::{
    prelude::{
        ChartBuilder, Circle, IntoDrawingArea, LabelAreaPosition, LineSeries, PathElement,
        RangedDateTime,
    },
    style::{Color, BLACK, BLUE, GREEN, RED, WHITE},
};
use tracing::{info, trace};

use crate::measure::Measurement;

/// Static name for the file where the latest plot is stored, to make sure that a new plot
/// always overwrites the older, thus avoiding the need for large storage capacity over time.
#[cfg(feature = "twitter")]
pub(crate) const PLOT_FILE_NAME: &str = "latest_plot.png";
#[cfg(not(feature = "twitter"))]
pub(crate) const PLOT_FILE_NAME: &str = "latest_plot.svg";

#[derive(Debug)]
pub(super) struct Plotter {
    out_dir: PathBuf,
}

impl Plotter {
    const PLOT_IMAGE_RESOLUTION: (u32, u32) = (1920, 1080); // or 1024x768 or 800x600

    #[tracing::instrument]
    pub(super) fn new<P: AsRef<Path> + Debug>(out_dir: P) -> Result<Self> {
        trace!("Creating new '{}'...", std::any::type_name::<Self>());

        match metadata(&out_dir) {
            Err(e) => bail!("failed to stat(2) the given path {:?}: {}", out_dir, e),
            Ok(md) => {
                if !md.is_dir() {
                    bail!("the given path {:?} is not a directory", out_dir)
                }
                let plot_path = out_dir.as_ref().join(PLOT_FILE_NAME);
                if let Err(err) = remove_file(&plot_path) {
                    if !matches!(err.kind(), ErrorKind::NotFound) {
                        bail!("failed to unlink(2) file {:?}: {}", plot_path, err)
                    } else {
                        trace!("Failed to unlink(2) file {:?}: {}", plot_path, err);
                    }
                }
            }
        };

        Ok(Self {
            out_dir: out_dir.as_ref().to_owned(),
        })
    }

    fn datetime_range(
        &self,
        data: &[(DateTime<Local>, Measurement)],
    ) -> Result<RangedDateTime<DateTime<Local>>> {
        let first = data
            .first()
            .ok_or_else(|| anyhow!("failed to retrieve first stored value"))?
            .0;
        let last = data
            .last()
            .ok_or_else(|| anyhow!("failed to retrieve last stored value"))?
            .0;

        if first == last {
            bail!("only one value is stored; no point to plot it");
        }
        Ok((first..last).into())
    }

    fn mbps_range(&self, data: &[(DateTime<Local>, Measurement)]) -> Range<f64> {
        let mut max = f64::MIN;
        for (_, m) in data {
            max = max.max(m.download_speed.max(m.upload_speed));
        }
        0f64..(if max < 100. {
            (max / 10.).ceil() * 10.
        } else {
            (max / 100.).ceil() * 100.
        })
    }

    fn ping_range(&self, data: &[(DateTime<Local>, Measurement)]) -> Range<f64> {
        let mut max = f64::MIN;
        for (_, m) in data {
            max = max.max(m.ping_latency);
        }
        0f64..(max * 1.2)
    }

    #[tracing::instrument(skip(self, data))]
    pub(super) async fn plot(&self, data: Vec<(DateTime<Local>, Measurement)>) {
        if data.len() < 2 {
            info!("Skipping plot since # measurements = {}", data.len());
            return;
        }

        let plot_file_name = self.out_dir.join(PLOT_FILE_NAME);
        let backend = {
            #[cfg(feature = "twitter")]
            {
                // If the "twitter" Cargo feature is enabled, plots are PNG which are supported by
                // the Twitter API.
                BitMapBackend::new(&plot_file_name, Self::PLOT_IMAGE_RESOLUTION).into_drawing_area()
            }
            #[cfg(not(feature = "twitter"))]
            {
                // If the "twitter" Cargo feature is disabled, plots are SVG, which are better.
                SVGBackend::new(&plot_file_name, Self::PLOT_IMAGE_RESOLUTION).into_drawing_area()
            }
        };
        backend
            .fill(&WHITE)
            .expect("failed to fill backend with WHITE");

        let datetime_range = self.datetime_range(&data).unwrap();
        let mbps_range = self.mbps_range(&data);
        let ping_range = self.ping_range(&data);

        let mut chart = ChartBuilder::on(&backend)
            .caption("netspeedmon measurements", ("sans-serif", 30))
            .set_label_area_size(LabelAreaPosition::Left, 50)
            .set_label_area_size(LabelAreaPosition::Bottom, 40)
            .set_label_area_size(LabelAreaPosition::Right, 50)
            .margin(5)
            .build_cartesian_2d(datetime_range.clone(), mbps_range)
            .expect("failed to build 2D-Cartesian")
            .set_secondary_coord(datetime_range, ping_range);
        // Draw primary axes
        chart
            .configure_mesh()
            .disable_x_mesh()
            .x_desc("Time")
            .y_desc("Bandwidth (Megabits Per Second)")
            .draw()
            .expect("failed to draw mesh and/or primary axes");
        // Draw secondary axes
        chart
            .configure_secondary_axes()
            .y_desc("Ping Latency (milliseconds)")
            .draw()
            .expect("failed to draw secondary axes");

        // Draw points & time series for download speed on primary axes
        chart
            .draw_series(LineSeries::new(
                data.iter().map(|(ts, m)| (*ts, m.download_speed)),
                &BLUE,
            ))
            .expect("failed to draw download speed series on primary axes")
            .label("Download (Mbps)")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLUE));
        chart
            .draw_series(
                data.iter()
                    .map(|(ts, m)| Circle::new((*ts, m.download_speed), 3, BLUE.filled())),
            )
            .expect("failed to draw download speed points on primary axes");

        // Draw points & time series for upload speed on primary axes
        chart
            .draw_series(LineSeries::new(
                data.iter().map(|(ts, m)| (*ts, m.upload_speed)),
                &GREEN,
            ))
            .expect("failed to draw upload speed series on primary axes")
            .label("Upload (Mbps)")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &GREEN));
        chart
            .draw_series(
                data.iter()
                    .map(|(ts, m)| Circle::new((*ts, m.upload_speed), 3, GREEN.filled())),
            )
            .expect("failed to draw upload speed points on primary axes");

        // Draw points & time series for ping latency on secondary axes
        chart
            .draw_secondary_series(LineSeries::new(
                data.iter().map(|(ts, m)| (*ts, m.ping_latency)),
                &RED,
            ))
            .expect("failed to draw ping latency on secondary axes")
            .label("Ping Latency (ms)")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));
        chart
            .draw_secondary_series(
                data.iter()
                    .map(|(ts, m)| Circle::new((*ts, m.ping_latency), 3, RED.filled())),
            )
            .expect("failed to draw ping latency points on secondary axes");

        // Draw labels/legend
        chart
            .configure_series_labels()
            .border_style(&BLACK)
            .background_style(&WHITE.mix(0.8))
            .draw()
            .expect("failed to draw series labels");

        // Save it to the local disk
        backend.present().expect("failed to write plot to file");
        trace!("Saved new plot to local disk");
    }
}
