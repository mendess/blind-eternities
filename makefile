install:
	cargo build --release --bin spark $(EXTRA_ARGS)
	sudo install --strip ./target/release/spark /usr/bin/
