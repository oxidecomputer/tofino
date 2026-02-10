// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

const SRC_FILE: &str = "src/c/pci.c";

fn main() {
    println!("cargo:rerun-if-changed={}", SRC_FILE);
    cc::Build::new().file(SRC_FILE).compile("pci");
}
