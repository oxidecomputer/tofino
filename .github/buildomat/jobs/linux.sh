#!/bin/bash
#:
#: name = "linux"
#: variety = "basic"
#: target = "ubuntu-20.04"
#: rust_toolchain = "stable"
#:

set -o errexit
set -o pipefail
set -o xtrace

banner "build"
cargo build --release

banner "check"
cargo fmt -- --check
cargo clippy -- --deny warnings
