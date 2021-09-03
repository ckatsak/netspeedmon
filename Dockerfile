# TODO: Care for some 'speedtest' binary currently missing; hence, unfinished

FROM rust:1.54.0-bullseye AS builder
ARG VERSION
LABEL stage=netspeedmon_builder

WORKDIR /src
COPY . .
RUN apt-get update -y && apt-get install -y cmake libfreetype6-dev \
	&& make release-all-features


FROM debian:bullseye-slim
ARG VERSION
LABEL maintainer="ckatsak@gmail.com" version=$VERSION

COPY --from=builder /src/target/release/netspeedmon /nsm/netspeedmon
COPY --from=builder /src/conf/default.json /nsm/conf/default.json

RUN apt-get update -y && apt-get install -y libfreetype6-dev \
	&& rm -rf /var/lib/apt/lists/* && mkdir -vp /var/netspeedmon
ENV RUST_LOG="netspeedmon=debug" RUST_BACKTRACE=1

ENTRYPOINT ["/nsm/netspeedmon"]
CMD ["--config", "/nsm/conf/default.json"]
