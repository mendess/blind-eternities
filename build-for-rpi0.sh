#!/bin/bash

set -e
if ! hash cross; then
    echo "Installing cargo cross..."
    cargo install cross --git https://github.com/cross-rs/cross
fi
if ! pgrep docker >/dev/null; then
    echo "Starting docker..."
    sudo systemctl start docker
fi

target=goblinww
case "$1" in
    mirrodin)
        target=mirrodin
        ;;
esac
set -x
cross build --target arm-unknown-linux-gnueabihf --bin spark --release
spark rsync av ./target/arm-unknown-linux-gnueabihf/release/spark $target:
spark ssh $target -- sudo install spark /usr/bin
spark ssh $target -- spark msg reload
