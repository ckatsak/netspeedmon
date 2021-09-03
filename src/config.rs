use std::time::Duration;

use anyhow::{Context, Result};
use clap::{
    crate_authors, crate_description, crate_license, crate_name, crate_version, App, AppSettings,
    Arg, ValueHint,
};
use config::File;
use serde::Deserialize;

use crate::exporters::database;
#[cfg(feature = "http")]
use crate::exporters::http;
#[cfg(feature = "twitter")]
use crate::exporters::twitter;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(with = "humantime_serde", alias = "Period")]
    pub(crate) period: Duration,
    #[serde(alias = "Measurer")]
    pub(crate) measurer: Option<String>,
    #[serde(default, alias = "StdOut", alias = "STDOUT")]
    pub(crate) stdout: bool,
    #[cfg(feature = "twitter")]
    #[serde(rename = "twitter", alias = "Twitter")]
    pub(crate) twitter_config: Option<twitter::Config>,
    #[cfg(feature = "http")]
    #[serde(rename = "http", alias = "HTTP")]
    pub(crate) http_config: Option<http::Config>,
    #[serde(rename = "database", alias = "db", alias = "Database")]
    pub(crate) db_config: Option<database::Config>,
}

impl Config {
    #[tracing::instrument]
    pub fn parse() -> Result<Self> {
        let matches = App::new(crate_name!())
            .setting(AppSettings::ColoredHelp)
            .author(crate_authors!())
            .license(crate_license!())
            .about(crate_description!())
            .version(crate_version!())
            .arg(
                Arg::new("config")
                    .short('c')
                    .long("config")
                    .about("Path to configuration file")
                    .required(true)
                    .takes_value(true)
                    .value_name("FILE")
                    .value_hint(ValueHint::FilePath)
                    .long_about("Path to configuration file. Examples are available in the examples directory."), // TODO
            )
            .long_about(crate_description!()) // TODO
            .get_matches();

        let config_path = matches.value_of("config").unwrap(); // SAFETY checked by clap

        let mut c = ::config::Config::default();
        c.merge(File::with_name(config_path).required(true))
            .with_context(|| format!("failed to merge config file '{}'", config_path))?;

        c.try_into().with_context(|| {
            "failed to convert '::config::Config' to 'netspeedmon::config::Config'"
        })
    }
}
