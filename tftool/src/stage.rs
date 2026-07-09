// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2026 Oxide Computer Company

//! Dump raw register words from a MAU stage, for offline analysis (e.g.
//! diffing live state against the values a tofino2.bin programs).

use anyhow::{Result, bail};

use crate::*;

const PIPE_BASE: u32 = 0x0400_0000;
const PIPE_STRIDE: u32 = 0x0100_0000;
const STAGE_STRIDE: u32 = 0x8_0000;

fn dump(
    ctx: &mut Tofino,
    pipe: u32,
    stage: u32,
    from: &str,
    words: u32,
) -> Result<()> {
    if pipe > 3 {
        bail!("pipe must be 0-3");
    }
    if stage > 19 {
        bail!("stage must be 0-19");
    }
    let from = parse_val(from)?;
    if from % 4 != 0 {
        bail!("offset must be 4-byte aligned");
    }
    if from as u64 + words as u64 * 4 > STAGE_STRIDE as u64 {
        bail!("window extends past the stage's {STAGE_STRIDE:#x} bytes");
    }

    let base = PIPE_BASE + pipe * PIPE_STRIDE + stage * STAGE_STRIDE + from;
    for i in 0..words {
        let addr = base + i * 4;
        let value = ctx.pci.read4(addr)?;
        println!("{addr:08x} {value:08x}");
    }
    Ok(())
}

pub fn stage_command(ctx: &mut Tofino, cmd: StageCommands) -> Result<()> {
    match cmd {
        StageCommands::Dump { pipe, stage, from, words } => {
            dump(ctx, pipe, stage, &from, words)
        }
    }
}
