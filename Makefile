default: build

build:
	cargo build --release

dev:
	cargo run

test:
	cargo test

fmt:
	cargo fmt

install: build
	# TODO make Nix flake
	cp target/release/knowledge-mcp ~/.local/bin/knowledge-mcp
