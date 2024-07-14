#!/bin/bash

set -e


extra_args=()
case "$(hostname)" in
    tolaria|weatherlight)
        extra_args+=("--features" "music-ctl")
        ;;
    *)
        read -r -p "Enable music control? [N/y] "
        case "$REPLY" in
            y|Y|yes|Yes)
                extra_args+=("--features" "music-ctl")
                ;;
        esac
        ;;
esac

cargo build -p spark --bin spark --release "${extra_args[@]}"

sudo install ./target/release/spark /usr/bin
