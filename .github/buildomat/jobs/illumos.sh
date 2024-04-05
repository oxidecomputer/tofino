#!/bin/bash
#:
#: name = "illumos"
#: variety = "basic"
#: target = "helios-latest"
#: rust_toolchain = "stable"
#: output_rules = [
#:   "/work/*",
#: ]
#:

set -o errexit
set -o pipefail
set -o xtrace

banner "build"
cargo build --release

banner "check"
cargo fmt -- --check
cargo clippy -- --deny warnings

mkdir -p /work/$x
mv target/release/tftool /work/
