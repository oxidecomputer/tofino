// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2026 Oxide Computer Company

//! In-situ write/readback test of a PHV container's datapath.
//!
//! The PHV pipeline itself has no software read/write port, so this test
//! goes through the datapath: a test pattern is injected by installing a
//! `deposit-field` instruction in the container's ALU at a chosen write
//! stage (on the always-run line, so it executes for every ingress packet),
//! and read back with the MAU snapshot machinery, which latches the PHV as
//! the packet flows through each downstream stage. Comparing the capture at
//! each stage against the injected pattern shows exactly where bits are
//! lost.
//!
//! Deposit-field's small-constant source plus its barrel rotate covers a
//! proper memtest pattern set: all-zeros, all-ones (const -1), a walking
//! one (const 1 rotated) and a walking zero (const -2 rotated).
//!
//! Requires packets to be flowing through the pipe (each pattern waits for
//! a packet to trigger the snapshot).

use anyhow::{Result, bail};
use colored::Colorize;
use rust_rpi::RegisterInstance;

use crate::imem::{self, Kind, PhvAlu};
use crate::*;

/// Snapshot FSM states (pipe_snapshot_fsm_state_t in bf-drivers).
const FSM_PASSIVE: u32 = 0;
const FSM_ARMED: u32 = 1;

const ALWAYS_RUN_LINE: u32 = 31;
const INGRESS: u32 = 0;

struct Pattern {
    name: String,
    val: i32,
    rot: u32,
    expect: u32,
}

fn patterns(size: u32, quick: bool) -> Vec<Pattern> {
    let mask = u32::MAX >> (32 - size);
    let mut out = vec![
        Pattern { name: "all-0".into(), val: 0, rot: 0, expect: 0 },
        Pattern { name: "all-1".into(), val: -1, rot: 0, expect: mask },
    ];
    if quick {
        return out;
    }
    for bit in 0..size {
        // effective value = val rotated right by rot (within `size` bits)
        let rot = (size - bit) % size;
        out.push(Pattern {
            name: format!("walk1-{bit:02}"),
            val: 1,
            rot,
            expect: 1 << bit,
        });
        out.push(Pattern {
            name: format!("walk0-{bit:02}"),
            val: -2,
            rot,
            expect: mask & !(1 << bit),
        });
    }
    out
}

fn dp(pipe: u32, stage: u32) -> Result<regs::DpInstance, rust_rpi::OutOfRange> {
    Ok(regs::Client::default().pipes(pipe)?.mau(stage)?.dp())
}

/// dp.imem_word_read_override: per-stage enable for the always-run imem
/// line (bit 0 ingress, bit 1 egress).
///
/// XXX this register is missing from rsf/tf2.rsf (the Dp block skips from
/// 0x20488 to 0x204f4), so it is accessed by raw offset until the spec
/// gains it.
const IMEM_WORD_READ_OVERRIDE: u32 = 0x204a0;

fn override_addr(pipe: u32, stage: u32) -> Result<u32> {
    let d = dp(pipe, stage)?;
    Ok(d.addr + IMEM_WORD_READ_OVERRIDE)
}

fn override_read(ctx: &mut Tofino, pipe: u32, stage: u32) -> Result<u32> {
    ctx.pci.read4(override_addr(pipe, stage)?)
}

fn override_write(
    ctx: &mut Tofino,
    pipe: u32,
    stage: u32,
    value: u32,
) -> Result<()> {
    ctx.pci.write4(override_addr(pipe, stage)?, value)
}

/// Program the stage's snapshot trigger to match every packet. The match
/// registers are dual-rail (a match-on-0 and a match-on-1 plane per
/// container word); all-ones in both planes is don't-care. The register
/// arrays are laid out [word][plane], flattened.
fn set_match_any(ctx: &mut Tofino, pipe: u32, stage: u32) -> Result<()> {
    let m = dp(pipe, stage)?.snapshot_dp().snapshot_match();
    for idx in 0..64 {
        for plane in 0..2 {
            m.mau_snapshot_match_subword_32_b_lo(idx * 2 + plane)?
                .write_raw(ctx, 0xffff)?;
            m.mau_snapshot_match_subword_32_b_hi(idx * 2 + plane)?
                .write_raw(ctx, 0x1ffff)?;
            m.mau_snapshot_match_subword_8_b(idx * 2 + plane)?
                .write_raw(ctx, 0x1ff)?;
        }
    }
    for idx in 0..96 {
        for plane in 0..2 {
            m.mau_snapshot_match_subword_16_b(idx * 2 + plane)?
                .write_raw(ctx, 0x1ffff)?;
        }
    }
    Ok(())
}

fn fsm_set(ctx: &mut Tofino, pipe: u32, stage: u32, state: u32) -> Result<()> {
    dp(pipe, stage)?
        .snapshot_ctl()
        .mau_fsm_snapshot_cur_stateq(INGRESS)?
        .write_raw(ctx, state)?;
    Ok(())
}

fn fsm_get(ctx: &mut Tofino, pipe: u32, stage: u32) -> Result<u32> {
    let v: u32 = dp(pipe, stage)?
        .snapshot_ctl()
        .mau_fsm_snapshot_cur_stateq(INGRESS)?
        .read(ctx)?
        .into();
    Ok(v & 0x3)
}

/// Read the captured value of the container from a stage's snapshot capture
/// registers. Capture registers cover 20 containers per group; the arrays
/// are split in two halves of `groups_per_side` groups, laid out
/// [slot 0..19][group], with the group dimension padded to 4 for the
/// 16-bit class.
fn read_capture(
    ctx: &mut Tofino,
    alu: &PhvAlu,
    pipe: u32,
    stage: u32,
) -> Result<u32> {
    let per_side = alu.class.groups_per_side() * 20;
    let cap = alu.cap_index();
    let half = cap / per_side;
    let group = (cap % per_side) / 20;
    let slot = cap % 20;
    let group_dim = match alu.class.bits() {
        16 => 4,
        _ => 2,
    };
    let flat = slot * group_dim + group;
    let c = dp(pipe, stage)?.snapshot_dp().snapshot_capture(half)?;
    let value = match alu.class.bits() {
        32 => {
            let lo: u32 =
                c.mau_snapshot_capture_subword_32_b_lo(flat)?.read(ctx)?.into();
            let hi: u32 =
                c.mau_snapshot_capture_subword_32_b_hi(flat)?.read(ctx)?.into();
            (lo & 0xffff) | (hi << 16)
        }
        16 => {
            let v: u32 =
                c.mau_snapshot_capture_subword_16_b(flat)?.read(ctx)?.into();
            v & 0xffff
        }
        _ => {
            let v: u32 =
                c.mau_snapshot_capture_subword_8_b(flat)?.read(ctx)?.into();
            v & 0xff
        }
    };
    Ok(value)
}

/// Wait for a stage's snapshot FSM to leave the armed state (i.e. a packet
/// has triggered and been captured).
fn wait_trigger(
    ctx: &mut Tofino,
    pipe: u32,
    stage: u32,
    timeout: std::time::Duration,
) -> Result<()> {
    let start = std::time::Instant::now();
    loop {
        let state = fsm_get(ctx, pipe, stage)?;
        if state != FSM_ARMED && state != FSM_PASSIVE {
            return Ok(());
        }
        if start.elapsed() > timeout {
            bail!(
                "snapshot did not trigger in {timeout:?}: \
                 is traffic flowing through pipe {pipe}?"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

struct TestState {
    pipe: u32,
    write_stage: u32,
    through: u32,
    imem_index: u32,
    saved_override: u32,
}

/// Best-effort restoration of everything the test touches: the injected
/// imem word, imem_word_read_override, and the snapshot FSMs. The snapshot
/// trigger/config registers are owned by whoever arms a snapshot next (dpd
/// rewrites them when swadm snapshot is used), so they are left as-is.
fn restore(ctx: &mut Tofino, alu: &PhvAlu, st: &TestState) {
    if let Err(e) =
        imem::write_word(ctx, alu, st.pipe, st.write_stage, st.imem_index, 0)
    {
        eprintln!("restore: clearing injected imem word failed: {e}");
    }
    if let Err(e) =
        override_write(ctx, st.pipe, st.write_stage, st.saved_override)
    {
        eprintln!("restore: imem_word_read_override failed: {e}");
    }
    for stage in st.write_stage..=st.through {
        if let Err(e) = fsm_set(ctx, st.pipe, stage, FSM_PASSIVE) {
            eprintln!("restore: fsm passive stage {stage} failed: {e}");
        }
    }
}

fn run_test(
    ctx: &mut Tofino,
    alu: &PhvAlu,
    st: &TestState,
    quick: bool,
    timeout: std::time::Duration,
) -> Result<()> {
    let size = alu.class.bits();
    let stages: Vec<u32> = (st.write_stage..=st.through).collect();

    // Configure snapshots: timer off / match-any-thread mode, and a trigger
    // that matches every packet.
    for &stage in &stages {
        let d = dp(st.pipe, stage)?;
        d.snapshot_ctl().mau_snapshot_config().write_raw(ctx, 0)?;
        set_match_any(ctx, st.pipe, stage)?;
    }

    // Enable the always-run line in the write stage. The injected pattern
    // word is installed per-iteration below.
    override_write(ctx, st.pipe, st.write_stage, st.saved_override | 0x1)?;

    let mut header = format!("{:9} {:>8}", "PATTERN", "EXPECT");
    for &stage in &stages {
        header.push_str(&format!(" {:>8}", format!("s{stage}")));
    }
    println!("{header}");

    // Aggregate per-stage defect masks
    let mut stuck0 = vec![0u32; stages.len()];
    let mut stuck1 = vec![0u32; stages.len()];

    for pattern in patterns(size, quick) {
        let word = alu.encode_deposit_const(pattern.val, pattern.rot)?;
        imem::write_word(
            ctx,
            alu,
            st.pipe,
            st.write_stage,
            st.imem_index,
            word,
        )?;

        // Arm downstream first so the trigger chain is ready when the write
        // stage fires.
        for &stage in stages.iter().rev() {
            dp(st.pipe, stage)?
                .snapshot_ctl()
                .mau_snapshot_datapath_capture(INGRESS)?
                .write_raw(ctx, 0)?;
            fsm_set(ctx, st.pipe, stage, FSM_ARMED)?;
        }

        // Wait for the last stage to capture: it triggers via the
        // previous-stage chain, so once it has fired every stage in the
        // range holds the same packet.
        wait_trigger(ctx, st.pipe, st.through, timeout)?;

        let mut row = format!("{:9} {:>8x}", pattern.name, pattern.expect);
        for (i, &stage) in stages.iter().enumerate() {
            let got = read_capture(ctx, alu, st.pipe, stage)?;
            // pad before colorizing: ANSI escapes would count against the
            // format width
            if got == pattern.expect {
                row.push_str(&format!(" {}", format!("{:>8}", "ok").green()));
            } else {
                row.push_str(&format!(" {}", format!("{:>8x}", got).red()));
                stuck0[i] |= pattern.expect & !got;
                stuck1[i] |= got & !pattern.expect;
            }
        }
        println!("{row}");

        for &stage in &stages {
            fsm_set(ctx, st.pipe, stage, FSM_PASSIVE)?;
        }
    }

    println!();
    let mut any = false;
    for (i, &stage) in stages.iter().enumerate() {
        if stuck0[i] != 0 || stuck1[i] != 0 {
            any = true;
            println!(
                "stage {stage}: bits captured 0 when written 1: {}, \
                 captured 1 when written 0: {}",
                format!("{:08x}", stuck0[i]).red(),
                format!("{:08x}", stuck1[i]).red(),
            );
        }
    }
    if !any {
        println!(
            "{}",
            "all patterns read back correctly at every stage".green()
        );
    } else {
        println!(
            "note: a defect between two capture points shows up at the \
             first bad stage and every stage after it"
        );
    }
    Ok(())
}

fn test(
    ctx: &mut Tofino,
    phv: &str,
    pipe: u32,
    write_stage: u32,
    through: u32,
    timeout_ms: u64,
    quick: bool,
) -> Result<()> {
    if pipe > 3 {
        bail!("pipe must be 0-3");
    }
    if write_stage > 19 || through > 19 || write_stage > through {
        bail!("stages must satisfy write-stage <= through <= 19");
    }
    let alu = PhvAlu::parse(phv)?;
    if alu.kind != Kind::Normal {
        bail!(
            "{} is a mocha/dark container: its ALU cannot source immediate \
             patterns; test the normal container that shares its group",
            alu.name
        );
    }

    let imem_index = alu.index(ALWAYS_RUN_LINE);
    let (_, existing) =
        imem::read_word(ctx, &alu, pipe, write_stage, imem_index)?;
    if existing != 0 {
        bail!(
            "{}'s always-run imem word in stage {write_stage} is in use \
             (value {existing:08x}, likely a compiler always-run action); \
             pick a different --write-stage",
            alu.name
        );
    }
    let saved_override = override_read(ctx, pipe, write_stage)?;

    println!(
        "write/readback test of {} in pipe {pipe}: inject at stage \
         {write_stage}, capture through stage {through}",
        alu.name
    );
    println!(
        "{}\n",
        format!(
            "WARNING: {} is overwritten for every ingress packet in pipe \
             {pipe} while this test runs",
            alu.name
        )
        .yellow()
    );

    let st =
        TestState { pipe, write_stage, through, imem_index, saved_override };
    let result = run_test(
        ctx,
        &alu,
        &st,
        quick,
        std::time::Duration::from_millis(timeout_ms),
    );
    restore(ctx, &alu, &st);
    result
}

pub fn phv_command(ctx: &mut Tofino, cmd: PhvCommands) -> Result<()> {
    match cmd {
        PhvCommands::Test {
            phv,
            pipe,
            write_stage,
            through,
            timeout_ms,
            quick,
        } => test(ctx, &phv, pipe, write_stage, through, timeout_ms, quick),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_encodings_decode_correctly() {
        let w3 = PhvAlu::parse("W3").unwrap();
        for p in patterns(32, false) {
            let word = w3.encode_deposit_const(p.val, p.rot).unwrap();
            let instr = word & ((1 << w3.instr_bits()) - 1);
            let decoded = w3.decode_instr(instr);
            let expect = format!("deposit-field W3(0..31), {}", p.expect);
            assert!(
                decoded.starts_with(&expect),
                "{}: decoded {:?}, expected {:?}",
                p.name,
                decoded,
                expect
            );
            // color set, parity even over instr+color
            assert_eq!((word >> w3.instr_bits()) & 1, 1, "{}", p.name);
            let parity = (word >> (w3.instr_bits() + 1)) & 1;
            assert_eq!(
                (instr.count_ones() + 1 + parity) & 1,
                0,
                "{}: bad parity",
                p.name
            );
        }
    }

    #[test]
    fn pattern_encodings_16bit() {
        let h20 = PhvAlu::parse("H20").unwrap();
        for p in patterns(16, false) {
            let word = h20.encode_deposit_const(p.val, p.rot).unwrap();
            let instr = word & ((1 << h20.instr_bits()) - 1);
            let decoded = h20.decode_instr(instr);
            let expect = format!("deposit-field H20(0..15), {}", p.expect);
            assert!(
                decoded.starts_with(&expect),
                "{}: decoded {:?}, expected {:?}",
                p.name,
                decoded,
                expect
            );
        }
    }

    #[test]
    fn capture_indexing() {
        // W3: class uid 3 -> half 0, group 0, slot 3
        let w3 = PhvAlu::parse("W3").unwrap();
        assert_eq!(w3.cap_index(), 3);
        // MW8: W group 2 -> half 1 (W groups split 2/2), slot 12
        let mw8 = PhvAlu::parse("MW8").unwrap();
        assert_eq!(mw8.cap_index(), 52);
        // MH6: H group 1 (3 groups per side), mocha slot 2 -> 20 + 14
        let mh6 = PhvAlu::parse("MH6").unwrap();
        assert_eq!(mh6.cap_index(), 34);
    }
}
