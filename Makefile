# NOTE: In hindsight, this mostly serves as a bunch of notes rather than an
# actual Makefile -_-

CARGO  ?= cargo
DOCKER ?= docker

IMG = ckatsak/netspeedmon
VERSION := 0.0.3


# TODO: A 'speedtest' binary is not included in the image for now;
# containerization is therefore not "ready" yet.
.PHONY: docker-native docker-build-native
docker-build:
	$(DOCKER) build --no-cache -f ./Dockerfile \
		--build-arg VERSION=$(VERSION) \
		-t $(IMG):$(VERSION) .
	$(DOCKER) image prune --force --filter label=stage=netspeedmon_builder

.PHONY: clean clean-docker
clean:
	$(CARGO) clean
clean-docker:
	-$(DOCKER) rmi $(IMG):$(VERSION)


# Valid Cargo feature combinations
FEAT_STDOUT       = --no-default-features --features=
FEAT_HTTP         = --no-default-features --features=http
FEAT_HTTP_PLOT    = --no-default-features --features=http,plot  # <-- default
FEAT_TWITTER      = --no-default-features --features=twitter
FEAT_TWITER_PLOT  = --no-default-features --features=twitter,plot
FEAT_HTTP_TWITTER = --no-default-features --features=http,twitter
FEAT_ALL          = --all-features
# All of the above can be combined with the `speedtestr` Cargo feature, to
# allow an alternative `Measurer` to be configured via the configuration file.
# This results to a total of 14 Cargo feature combinations.

.PHONY: release-stdout release-http release release-http-plot release-twitter \
	release-twitter-plot release-all-features
release-stdout:
	$(CARGO) b $(FEAT_STDOUT) --release
release-http:
	$(CARGO) b $(FEAT_HTTP) --release
release-http-plot:
	$(CARGO) b $(FEAT_HTTP_PLOT) --release
release-twitter:
	$(CARGO) b $(FEAT_TWITTER) --release
release-twitter-plot:
	$(CARGO) b $(FEAT_TWITER_PLOT) --release
release-http-twitter:
	$(CARGO) b $(FEAT_HTTP_TWITTER) --release
release-all-features:
	$(CARGO) b $(FEAT_ALL) --release

.PHONY: clippy-all-combs
clippy-all-combs: clean
	$(CARGO) clippy $(FEAT_STDOUT) -- --D warnings
	$(CARGO) clean
	$(CARGO) clippy $(FEAT_HTTP) -- --D warnings
	$(CARGO) clean
	$(CARGO) clippy $(FEAT_HTTP_PLOT) -- --D warnings
	$(CARGO) clean
	$(CARGO) clippy $(FEAT_TWITTER) -- --D warnings
	$(CARGO) clean
	$(CARGO) clippy $(FEAT_TWITER_PLOT) -- --D warnings
	$(CARGO) clean
	$(CARGO) clippy $(FEAT_HTTP_TWITTER) -- --D warnings
	$(CARGO) clean
	$(CARGO) clippy $(FEAT_ALL) -- --D warnings
	$(CARGO) clean
