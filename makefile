debug:
	RUSTFLAGS="--cfg tokio_unstable" cargo build 

release:
	RUSTFLAGS="--cfg tokio_unstable" cargo build --release

clean:
	cargo clean

