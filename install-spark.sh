#!/bin/bash

set -e

read -r -p "Enable music control? [N/y] "

extra_args=()
case "$REPLY" in
    y|Y|yes|Yes)
        extra_args+=("--features" "music-ctl")
        ;;
esac

cargo build --release "${extra_args[@]}"

sudo install ./target/release/spark /usr/bin
