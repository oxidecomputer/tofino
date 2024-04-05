// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};

use crate::*;

#[derive(Debug)]
enum DrFieldOffsets {
    Ctrl = 0x00,
    BaseAddrLow = 0x04,
    BaseAddrHigh = 0x08,
    LimitAddrLow = 0x0c,
    LimitAddrHigh = 0x10,
    Size = 0x14,
    HeadPtr = 0x18,
    TailPtr = 0x1c,
    RingTimeout = 0x20,
    DataTimeout = 0x24,
    Status = 0x28,
    // tofino 2 additions:
    // empty_int_time = 0x2c
    // empty_int_count = 0x30
}

impl std::ops::Add<DrFieldOffsets> for u32 {
    type Output = Self;

    fn add(self, b: DrFieldOffsets) -> u32 {
        b as u32 + self
    }
}

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

// This returns a mapping of the DR names to their offsets in the register
// space.  It would be nice if we could look these up via the register names in
// the .bin file, but the Tofino1 definitions are incomplete.  When we stop
// using the Wedge system, this can be reworked to drop the hardcoded values.

fn get_drs() -> BTreeMap<String, u32> {
    let mut m = BTreeMap::new();
    m.insert("fm_pkt_0".to_string(), 0x300400u32);
    m.insert("fm_pkt_1 ".to_string(), 0x300434u32);
    m.insert("fm_pkt_2".to_string(), 0x300468u32);
    m.insert("fm_pkt_3".to_string(), 0x30049cu32);
    m.insert("fm_pkt_4".to_string(), 0x3004d0u32);
    m.insert("fm_pkt_5".to_string(), 0x300504u32);
    m.insert("fm_pkt_6".to_string(), 0x300538u32);
    m.insert("fm_pkt_7".to_string(), 0x30056cu32);
    m.insert("fm_lrt".to_string(), 0x200900u32);
    m.insert("fm_idle".to_string(), 0x200980u32);
    m.insert("fm_learn".to_string(), 0x280600u32);
    m.insert("fm_diag".to_string(), 0x200a00u32);
    m.insert("tx_pipe_inst_list_0".to_string(), 0x200600u32);
    m.insert("tx_pipe_inst_list_1".to_string(), 0x200634u32);
    m.insert("tx_pipe_inst_list_2".to_string(), 0x200668u32);
    m.insert("tx_pipe_inst_list_3".to_string(), 0x20069cu32);
    m.insert("tx_pipe_write_block".to_string(), 0x200800u32);
    m.insert("tx_pipe_read_block".to_string(), 0x200880u32);
    m.insert("tx_que_write_list".to_string(), 0x280400u32);
    m.insert("tx_pkt_0".to_string(), 0x300100u32);
    m.insert("tx_pkt_1".to_string(), 0x300134u32);
    m.insert("tx_pkt_2".to_string(), 0x300168u32);
    m.insert("tx_pkt_3".to_string(), 0x30019cu32);
    m.insert("tx_mac_stat".to_string(), 0x180200u32);
    m.insert("rx_pkt_0".to_string(), 0x300600u32);
    m.insert("rx_pkt_1".to_string(), 0x300634u32);
    m.insert("rx_pkt_2".to_string(), 0x300668u32);
    m.insert("rx_pkt_3".to_string(), 0x30069cu32);
    m.insert("rx_pkt_4".to_string(), 0x3006d0u32);
    m.insert("rx_pkt_5".to_string(), 0x300704u32);
    m.insert("rx_pkt_6".to_string(), 0x300738u32);
    m.insert("rx_pkt_7".to_string(), 0x30076cu32);
    m.insert("rx_lrt".to_string(), 0x200940u32);
    m.insert("rx_idle".to_string(), 0x2009c0u32);
    m.insert("rx_learn".to_string(), 0x280640u32);
    m.insert("rx_diag".to_string(), 0x200a40u32);
    m.insert("cmp_pipe_inst_list_0".to_string(), 0x200700u32);
    m.insert("cmp_pipe_inst_list_1".to_string(), 0x200734u32);
    m.insert("cmp_pipe_inst_list_2".to_string(), 0x200768u32);
    m.insert("cmp_pipe_inst_list_3".to_string(), 0x20079cu32);
    m.insert("cmp_que_write_list".to_string(), 0x280440u32);
    m.insert("cmp_pipe_write_blk".to_string(), 0x200840u32);
    m.insert("cmp_pipe_read_blk".to_string(), 0x2008c0u32);
    m.insert("cmp_mac_stat".to_string(), 0x180240u32);
    m.insert("cmp_tx_pkt_0".to_string(), 0x300200u32);
    m.insert("cmp_tx_pkt_1".to_string(), 0x300234u32);
    m.insert("cmp_tx_pkt_2".to_string(), 0x300268u32);
    m.insert("cmp_tx_pkt_3".to_string(), 0x30029cu32);
    m.insert("tx_mac_write_block".to_string(), 0x180280u32);
    m.insert("tx_que_write_list_1".to_string(), 0x280480u32);
    m.insert("tx_que_read_block_0".to_string(), 0x280500u32);
    m.insert("tx_que_read_block_1".to_string(), 0x280580u32);
    m.insert("cmp_mac_write_block".to_string(), 0x1802c0u32);
    m.insert("cmp_que_write_list_1".to_string(), 0x2804c0u32);
    m.insert("cmp_que_read_block_0".to_string(), 0x280540u32);
    m.insert("cmp_que_read_block_1".to_string(), 0x2805c0u32);
    m
}

fn list() {
    println!("{:21} {:6}", "NAME", "OFFSET");
    for (name, offset) in get_drs() {
        println!("{:21} 0x{:>06x}", name, offset);
    }
}

fn read_dr(ctx: &mut Tofino, offset: u32) -> Result<Dr> {
    let dr = Dr {
        ctrl: ctx.pci.read4(offset + DrFieldOffsets::Ctrl)?,
        base_addr_low: ctx.pci.read4(offset + DrFieldOffsets::BaseAddrLow)?,
        base_addr_high: ctx.pci.read4(offset + DrFieldOffsets::BaseAddrHigh)?,
        limit_addr_low: ctx.pci.read4(offset + DrFieldOffsets::LimitAddrLow)?,
        limit_addr_high: ctx
            .pci
            .read4(offset + DrFieldOffsets::LimitAddrHigh)?,
        size: ctx.pci.read4(offset + DrFieldOffsets::Size)?,
        head_ptr: ctx.pci.read4(offset + DrFieldOffsets::HeadPtr)?,
        tail_ptr: ctx.pci.read4(offset + DrFieldOffsets::TailPtr)?,
        ring_timeout: ctx.pci.read4(offset + DrFieldOffsets::RingTimeout)?,
        data_timeout: ctx.pci.read4(offset + DrFieldOffsets::DataTimeout)?,
        status: ctx.pci.read4(offset + DrFieldOffsets::Status)?,
    };
    Ok(dr)
}

fn show(ctx: &mut Tofino, dr: String) -> Result<()> {
    let all = get_drs();
    if let Some(o) = all.get(&dr) {
        let dr = read_dr(ctx, *o)?;
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
    } else {
        Err(anyhow!("no such DR"))
    }
}

fn dump(ctx: &mut Tofino) -> Result<()> {
    println!(
        "{:21} {:8} {:16} {:16} {:>6} {:>6} {:8}",
        "NAME", "CTRL", "BASE", "LIMIT", "HEAD", "TAIL", "STATUS"
    );
    for (name, offset) in get_drs() {
        let dr = read_dr(ctx, offset)?;
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
        DrCommands::List => {
            list();
            Ok(())
        }
        DrCommands::Show { dr } => show(ctx, dr),
        DrCommands::Dump => dump(ctx),
    }
}
