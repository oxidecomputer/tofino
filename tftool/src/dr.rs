// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use anyhow::{Result, anyhow};
use paste::paste;
use rust_rpi::RegisterInstance;

use crate::*;

#[derive(Debug)]
struct Dr {
    ctrl: u32,
    base_addr_low: u32,
    base_addr_high: u32,
    limit_addr_low: u32,
    limit_addr_high: u32,
    size: u32,
    head_ptr: u32,
    tail_ptr: u32,
    ring_timeout: u32,
    data_timeout: u32,
    status: u32,
}

macro_rules! read_dr_common {
    ($dr:ident, $ctx:ident) => {
        Dr {
            ctrl: u32::from($dr.ctrl().read($ctx)?),
            base_addr_low: u32::from($dr.base_addr_low().read($ctx)?),
            base_addr_high: u32::from($dr.base_addr_high().read($ctx)?),
            limit_addr_low: u32::from($dr.limit_addr_low().read($ctx)?),
            limit_addr_high: u32::from($dr.limit_addr_high().read($ctx)?),
            size: u32::from($dr.size().read($ctx)?),
            head_ptr: u32::from($dr.head_ptr().read($ctx)?),
            tail_ptr: u32::from($dr.tail_ptr().read($ctx)?),
            ring_timeout: u32::from($dr.ring_timeout().read($ctx)?),
            data_timeout: u32::from($dr.data_timeout().read($ctx)?),
            status: u32::from($dr.status().read($ctx)?),
        }
    };
}

macro_rules! read_dr_idx {
    ($block:ident, $name:ident) => {
        paste! {
        fn [< read_ $name>](ctx: &mut Tofino, index: u32) -> Result<Dr> {
            let dr =
                regs::Client::default().device_select().$block().$name(index)?;
            Ok(read_dr_common!(dr, ctx))
        }
        }
    };
}

macro_rules! read_dr {
    ($block:ident, $name:ident) => {
        paste! {
        fn [<read_ $name>](ctx: &mut Tofino) -> Result<Dr> {
            let dr = regs::Client::default().device_select().$block().$name();
            Ok(read_dr_common!(dr, ctx))
        }
        }
    };
}

// TBUS descriptor rings
read_dr_idx!(tbc, tbc_tx_dr);
read_dr_idx!(tbc, tbc_cpl_dr);
read_dr_idx!(tbc, tbc_fm_dr);
read_dr_idx!(tbc, tbc_rx_dr);

// pipe instruction list rings
read_dr_idx!(pbc, pbc_il_tx_dr);
read_dr_idx!(pbc, pbc_il_cpl_dr);

// pipe instruction read/write block rings
read_dr!(pbc, pbc_rb_tx_dr);
read_dr!(pbc, pbc_rb_cpl_dr);
read_dr!(pbc, pbc_wb_tx_dr);
read_dr!(pbc, pbc_wb_cpl_dr);

#[allow(clippy::type_complexity)]
const DR_FNS: [(&str, fn(&mut Tofino) -> Result<Dr>); 36] = [
    ("tbus_tx_0", |ctx| read_tbc_tx_dr(ctx, 0)),
    ("tbus_tx_1", |ctx| read_tbc_tx_dr(ctx, 1)),
    ("tbus_tx_2", |ctx| read_tbc_tx_dr(ctx, 2)),
    ("tbus_tx_3", |ctx| read_tbc_tx_dr(ctx, 3)),
    ("tbus_cpl_0", |ctx| read_tbc_cpl_dr(ctx, 0)),
    ("tbus_cpl_1", |ctx| read_tbc_cpl_dr(ctx, 1)),
    ("tbus_cpl_2", |ctx| read_tbc_cpl_dr(ctx, 2)),
    ("tbus_cpl_3", |ctx| read_tbc_cpl_dr(ctx, 3)),
    ("tbus_fm_0", |ctx| read_tbc_fm_dr(ctx, 0)),
    ("tbus_fm_1", |ctx| read_tbc_fm_dr(ctx, 1)),
    ("tbus_fm_2", |ctx| read_tbc_fm_dr(ctx, 2)),
    ("tbus_fm_3", |ctx| read_tbc_fm_dr(ctx, 3)),
    ("tbus_fm_4", |ctx| read_tbc_fm_dr(ctx, 4)),
    ("tbus_fm_5", |ctx| read_tbc_fm_dr(ctx, 5)),
    ("tbus_fm_6", |ctx| read_tbc_fm_dr(ctx, 6)),
    ("tbus_fm_7", |ctx| read_tbc_fm_dr(ctx, 7)),
    ("tbus_rx_0", |ctx| read_tbc_rx_dr(ctx, 0)),
    ("tbus_rx_1", |ctx| read_tbc_rx_dr(ctx, 1)),
    ("tbus_rx_2", |ctx| read_tbc_rx_dr(ctx, 2)),
    ("tbus_rx_3", |ctx| read_tbc_rx_dr(ctx, 3)),
    ("tbus_rx_4", |ctx| read_tbc_rx_dr(ctx, 4)),
    ("tbus_rx_5", |ctx| read_tbc_rx_dr(ctx, 5)),
    ("tbus_rx_6", |ctx| read_tbc_rx_dr(ctx, 6)),
    ("tbus_rx_7", |ctx| read_tbc_rx_dr(ctx, 7)),
    ("pbc_il_tx_0", |ctx| read_pbc_il_tx_dr(ctx, 0)),
    ("pbc_il_tx_1", |ctx| read_pbc_il_tx_dr(ctx, 1)),
    ("pbc_il_tx_2", |ctx| read_pbc_il_tx_dr(ctx, 2)),
    ("pbc_il_tx_3", |ctx| read_pbc_il_tx_dr(ctx, 3)),
    ("pbc_il_cpl_0", |ctx| read_pbc_il_cpl_dr(ctx, 0)),
    ("pbc_il_cpl_1", |ctx| read_pbc_il_cpl_dr(ctx, 1)),
    ("pbc_il_cpl_2", |ctx| read_pbc_il_cpl_dr(ctx, 2)),
    ("pbc_il_cpl_3", |ctx| read_pbc_il_cpl_dr(ctx, 3)),
    ("pbc_rb_tx", |ctx| read_pbc_rb_tx_dr(ctx)),
    ("pbc_rb_cpl", |ctx| read_pbc_rb_cpl_dr(ctx)),
    ("pbc_wb_tx", |ctx| read_pbc_wb_tx_dr(ctx)),
    ("pbc_wb_cpl", |ctx| read_pbc_wb_cpl_dr(ctx)),
];

fn show(ctx: &mut Tofino, name: &str) -> Result<()> {
    let read_fn = DR_FNS
        .iter()
        .find(|(n, _f)| &name == n)
        .map(|(_n, f)| f)
        .ok_or(anyhow!(format!("no such descriptor ring: {}", name)))?;

    let dr = read_fn(ctx)?;
    println!("ctrl: {:08x}", dr.ctrl);
    println!("base_addr_low: {:08x}", dr.base_addr_low);
    println!("base_addr_high: {:08x}", dr.base_addr_high);
    println!("limit_addr_low: {:08x}", dr.limit_addr_low);
    println!("limit_addr_high: {:08x}", dr.limit_addr_high);
    println!("size: {:08x}", dr.size);
    println!("head_ptr: {:08x}", dr.head_ptr);
    println!("tail_ptr: {:08x}", dr.tail_ptr);
    println!("ring_timeout: {:08x}", dr.ring_timeout);
    println!("data_timeout: {:08x}", dr.data_timeout);
    println!("status: {:08x}", dr.status);
    Ok(())
}

fn dump(ctx: &mut Tofino) -> Result<()> {
    println!(
        "{:21} {:8} {:16} {:16} {:>6} {:>6} {:8}",
        "NAME", "CTRL", "BASE", "LIMIT", "HEAD", "TAIL", "STATUS"
    );
    for (name, read_fn) in DR_FNS {
        let dr = read_fn(ctx)?;
        let base = (dr.base_addr_high as u64) << 32 | dr.base_addr_low as u64;
        let limit =
            (dr.limit_addr_high as u64) << 32 | dr.limit_addr_low as u64;
        println!(
            "{:21} {:08x} {:016x} {:016x} {:>6x} {:>6x} {:08x}",
            name, dr.ctrl, base, limit, dr.head_ptr, dr.tail_ptr, dr.status
        );
        if limit - base != dr.size as u64 {
            println!("base->limit range doesn't match size of {}", dr.size);
        }
    }
    Ok(())
}

pub fn dr_command(ctx: &mut Tofino, cmd: DrCommands) -> Result<()> {
    match cmd {
        DrCommands::Show { dr } => show(ctx, &dr),
        DrCommands::Dump => dump(ctx),
    }
}
