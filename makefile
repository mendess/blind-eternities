install:
	cargo build --release --bin spark
	sudo install --strip ./target/release/spark /usr/bin/
