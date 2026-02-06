default: build

build:
	cargo build --release

dev:
	cargo run

test:
	cargo test

fmt:
	cargo fmt

install:
	# TODO make Nix flake
	cargo install --path .
