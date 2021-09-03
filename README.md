# netspeedmon

<!--[![Crates.io](https://img.shields.io/crates/v/netspeedmon.svg)](https://crates.io/crates/netspeedmon)-->
<!--[![docs.rs](https://docs.rs/netspeedmon/badge.svg)](https://docs.rs/netspeedmon)-->
[![GitHub License](https://img.shields.io/github/license/ckatsak/netspeedmon?style=flat)](LICENSE)
[![deps.rs](https://deps.rs/repo/github/ckatsak/netspeedmon/status.svg)](https://deps.rs/repo/github/ckatsak/netspeedmon)

Command line utility to periodically measure, plot and report network statistics.

<sup>**Disclaimer:** _Working, but not polished; developed mostly for educational purposes._</sup>

## Quick build

- **Minimum Supported Rust Version:** 1.54.0

- You may need to install `cmake` and the FreeType 2 font engine (packages `cmake` and `libfreetype6-dev` on Debian).

- Assuming Rust is already installed in the system, build the fully-featured (and thus heavier) `netspeedmon` binary by issuing:
```console
$ cargo build --all-features --release
```
or try any other valid combination of the supported Cargo features (also kinda enumerated in the included [Makefile](Makefile)) according to your needs (which can lead to slimmer binaries).

## Measuring

Measurements can be acquired either by calling an external `speedtest` binary with certain characteristics, or by using the [zpeters/speedtestr](https://github.com/zpeters/speedtestr) crate.

### Binary `speedtest`

This is the default `Measurer`.

It is assumed that a `speedtest` binary exists in `PATH`, which is run by `netspeedmon` in a manner similar to:
```console
$ speedtest --format json --accept-gdpr
```

It is assumed that this binary reports the results serialized in JSON format on stdout.
Binary's stderr is never parsed, although it is being logged as a warning when is is not empty.

Furthermore, it is assumed that, in `speedtest` binary's output on stdout, _at least_ the following information can be found:
- In case of a successful execution:
```json
{
    "ping": {
        "latency": float (milliseconds)
    },
    "download": {
        "bandwidth": int (bytes per second)
    },
    "upload": {
        "bandwidth": int (bytes per second)
    }
}
```
- In case of an unsuccessful execution:
```json
{
    "error": "string"
}
```
In both cases, these are then parsed by `netspeedmon`.

Ookla's [Speedtest CLI](https://www.speedtest.net/apps/cli) is obviously a great candidate for this, for now.

### Crate [`zpeters/speedtestr`](https://github.com/zpeters/speedtestr)

To use this crate, first make sure that the Cargo feature `zpeters` has been enabled during the build.

Then, mind to specify the alternative `measurer` in the configuration file.

## Plotting

Optional feature, using the [`plotters` crate](https://crates.io/crates/plotters).

To enable plotting, make sure that:
- the Cargo feature `plot` has been enabled during the build;
- a database `path` has been specified in the configuration file (which is otherwise optional).

Plot images are in PNG format if the `twitter` Cargo feature is enabled, or SVG otherwise.

Results are plotted only when at least 2 measurements are available.

## Reporting

Results can periodically be:
- stored by a database implementation (although only a naive in-memory implementation exists, for now) (this is necessary enable plotting the time series, but optionally otherwise);
- written to stdout (configurable through a boolean on the configuration file);
- served via HTTP, on the `/latest` and `/plot` endpoints (Cargo feature `http` required);
- tweeted to the configured Twitter account (Cargo feature `twitter` required).

Logs are sent to stderr.
Logging level is configurable through the `RUST_LOG` environment variable.

You may want to redirect stderr to make the output on stdout easier to look at.

## Configuration

`netspeedmon` is configured when its execution begins through the configuration file pointed by the command line flag (try passing `--help` for more).

Some examples of what a valid configuration file may look like are [included in this repository](./conf/).

It is advisable that the periodic check is not configured to take place too often.


## License

Distributed under the terms of the Apache License, Version 2.0.

For further details consult the included [LICENSE file](LICENSE) or http://www.apache.org/licenses/LICENSE-2.0.

Of course, any other library or binary that is or can be used along with this project may be subject to terms of some different license(s).
