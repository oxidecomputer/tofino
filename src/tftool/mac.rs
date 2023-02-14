// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use anyhow::{anyhow, Result};

use crate::common::*;
use crate::{read_register, Tofino};

// Each field contains one bit of state for each of 4 channels
struct Eth100GStatus {
    macsts_sigok: u8,
    macsts_txidle: u8,
    macsts_rxidle: u8,
    macsts_txgood: u8,
}

// Each field contains one bit of state for each of 8 channels
struct Eth400GStatus {
    macsts_lfault: u8,
    macsts_rfault: u8,
    macsts_ofault: u8,
    macsts_linkup: u8,
    macsts_sigok: u8,
    macsts_txidle: u8,
    macsts_rxidle: u8,
    macsts_txgood: u8,
}

#[allow(dead_code)]
struct Eth400GhIntStat {
    intr_lo_stat: u8,
    intr_hi_stat: u8,
}

fn eth100g_status(ctx: &mut Tofino) -> Result<Eth100GStatus> {
    let val = read_register(ctx, "eth100g_regs.eth100g_reg.eth_status", 1)?;
    Ok(Eth100GStatus {
        macsts_sigok: get_bits(&val, 0, 3) as u8,
        macsts_txidle: get_bits(&val, 4, 7) as u8,
        macsts_rxidle: get_bits(&val, 8, 11) as u8,
        macsts_txgood: get_bits(&val, 12, 15) as u8,
    })
}

fn eth400g_status(ctx: &mut Tofino, mac: u32) -> Result<Eth400GStatus> {
    let base = format!("eth400g_p{}.eth400g_mac", mac);
    let path0 = format!("{}.eth_status0", base);
    let path1 = format!("{}.eth_status1", base);
    let stat0 = read_register(ctx, &path0, 1)?;
    let stat1 = read_register(ctx, &path1, 1)?;

    Ok(Eth400GStatus {
        macsts_lfault: get_bits(&stat0, 0, 7) as u8,
        macsts_rfault: get_bits(&stat0, 8, 15) as u8,
        macsts_ofault: get_bits(&stat0, 16, 23) as u8,
        macsts_linkup: get_bits(&stat0, 24, 31) as u8,
        macsts_sigok: get_bits(&stat1, 0, 7) as u8,
        macsts_txidle: get_bits(&stat1, 8, 15) as u8,
        macsts_rxidle: get_bits(&stat1, 16, 23) as u8,
        macsts_txgood: get_bits(&stat1, 24, 31) as u8,
    })
}

fn eth400g_line(label: &str, val: u8) {
    println!(
        "{:6}\t{:1} {:1} {:1} {:1} {:1} {:1} {:1} {:1}",
        label,
        get_bit(val, 0),
        get_bit(val, 1),
        get_bit(val, 2),
        get_bit(val, 3),
        get_bit(val, 4),
        get_bit(val, 5),
        get_bit(val, 6),
        get_bit(val, 7),
    )
}

fn show_eth400g(ctx: &mut Tofino, mac: u32) -> Result<()> {
    let s = eth400g_status(ctx, mac)?;
    println!("{:6}\t    Channels", "");
    println!(
        "{:6}\t{:1} {:1} {:1} {:1} {:1} {:1} {:1} {:1}",
        "", 0, 1, 2, 3, 4, 5, 6, 7
    );
    eth400g_line("lfault", s.macsts_lfault);
    eth400g_line("rfault", s.macsts_rfault);
    eth400g_line("ofault", s.macsts_ofault);
    eth400g_line("linkup", s.macsts_linkup);
    eth400g_line("sigok", s.macsts_sigok);
    eth400g_line("txidle", s.macsts_txidle);
    eth400g_line("rxidle", s.macsts_rxidle);
    eth400g_line("txgood", s.macsts_txgood);
    Ok(())
}

fn show_all_eth400g(ctx: &mut Tofino) -> Result<()> {
    println!(
        "{:3} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "mac",
        "lfault",
        "rfault",
        "ofault",
        "linkup",
        "sigok",
        "txidle",
        "rxidle",
        "txgood"
    );

    for mac in 1..32 {
        let s = eth400g_status(ctx, mac)?;
        println!(
            "{:3} {:6x} {:6x} {:6x} {:6x} {:6x} {:6x} {:6x} {:6x}",
            mac,
            s.macsts_lfault,
            s.macsts_rfault,
            s.macsts_ofault,
            s.macsts_linkup,
            s.macsts_sigok,
            s.macsts_txidle,
            s.macsts_rxidle,
            s.macsts_txgood
        );
    }
    Ok(())
}

fn aux_line(label: &str, val: u8) {
    println!(
        "{:6}\t{:1} {:1} {:1} {:1}",
        label,
        get_bit(val, 0),
        get_bit(val, 1),
        get_bit(val, 2),
        get_bit(val, 3),
    )
}

fn show_aux(ctx: &mut Tofino) -> Result<()> {
    let s = eth100g_status(ctx)?;
    println!("{:6}\tChannels", "");
    println!("{:6}\t{:1} {:1} {:1} {:1}", "", 0, 1, 2, 3);
    aux_line("sigok", s.macsts_sigok);
    aux_line("txidle", s.macsts_txidle);
    aux_line("rxidle", s.macsts_rxidle);
    aux_line("txgood", s.macsts_txgood);
    Ok(())
}

pub fn status(ctx: &mut Tofino, mac: Option<String>) -> Result<()> {
    if let Some(mac) = mac {
        if mac.to_ascii_lowercase() == "aux"
            || mac.to_ascii_lowercase() == "cpu"
        {
            show_aux(ctx)
        } else if let Ok(mac) = mac.parse::<u32>() {
            show_eth400g(ctx, mac)
        } else {
            Err(anyhow!("invalid mac: {}", mac))
        }
    } else {
        show_all_eth400g(ctx)
    }
}
