.PHONY: build test clean

build:
	cargo build --package nft --target wasm32-unknown-unknown --release
	cp target/wasm32-unknown-unknown/release/nft.wasm ./

test:
	cargo test --package nft

clean:
	cargo clean --package nft
	rm -f nft.wasm 