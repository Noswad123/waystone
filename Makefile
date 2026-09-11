.PHONY: help fmt check test build release install smoke clean

help:
	@printf 'waystone targets:\n'
	@printf '  make fmt      Format Rust sources\n'
	@printf '  make check    Type-check Rust sources\n'
	@printf '  make test     Run unit tests\n'
	@printf '  make build    Build debug binary\n'
	@printf '  make release  Build release binary\n'
	@printf '  make install  Install binary and completions via install.sh\n'
	@printf '  make smoke    Run smoke test script\n'
	@printf '  make clean    Remove Cargo build artifacts\n'

fmt:
	cargo fmt

check:
	cargo check

test:
	cargo test

build:
	cargo build

release:
	cargo build --release

install:
	./install.sh

smoke:
	scripts/smoke-test

clean:
	cargo clean
