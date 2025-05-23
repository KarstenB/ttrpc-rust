all: debug test

#
# Build
#

.PHONY: debug
debug:
	cargo build --verbose --all-targets

.PHONY: release
release:
	cargo build --release

.PHONY: build
build: debug

#
# Tests and linters
#

.PHONY: test
test:
	cargo test --all-features --verbose
	
.PHONY: check
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: check-all
check-all:
	$(MAKE) check
	$(MAKE) -C compiler check
	$(MAKE) -C ttrpc-codegen check
