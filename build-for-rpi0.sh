#!/bin/bash

cross build --target arm-unknown-linux-gnueabihf --bin spark --release
spark rsync av ./target/arm-unknown-linux-gnueabihf/release/spark goblinww:
spark ssh goblinww -- sudo install spark /usr/bin
