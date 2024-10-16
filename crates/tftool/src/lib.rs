// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};

mod dr;
mod fuse;
mod mac;

const REGISTER_SIZE: usize = 72 * 1024 * 1024;

mod tofino_regs {
    use anyhow::{anyhow, Result};

    pub struct Node {
        pub size: u32,
    }
    pub struct RegMap {}

    impl RegMap {
        pub fn new() -> Result<Self> {
            Ok(RegMap {})
        }

        pub fn get_node(&self, _reg: &str) -> Result<&Node> {
            Err(anyhow!("no register map available"))
        }

        pub fn get_offset(&self, _reg: &str) -> Result<u32> {
            Err(anyhow!("no register map available"))
        }
    }

    pub fn get_children(_node: &Node) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Debug, Parser)]
pub enum TftoolCommand {
    /// Dump the content of the fuse registers.
    Fuse,

    #[clap(subcommand)]
    Dr(DrCommands),

    #[clap(subcommand)]
    Reg(RegCommands),

    #[clap(subcommand)]
    Mac(MacCommands),
}

/// Dump info about descriptor rings.
#[derive(Debug, Subcommand)]
pub enum DrCommands {
    /// List the descriptor rings and their offsets.
    List,

    /// Show the register values for a single descriptor ring.
    Show {
        /// The descriptor ring.
        dr: String,
    },

    /// Dump summary information for all descriptor rings.
    Dump,
}

/// Display MAC register state.
#[derive(Debug, Subcommand)]
pub enum MacCommands {
    /// Show the per-channel state for one or all MACs.
    Status {
        /// MACs to display: `aux`, `cpu`, or a number between 1-32.
        mac: Option<String>,
    },
}

/// Operate on Tofino registers.
#[derive(Debug, Subcommand)]
pub enum RegCommands {
    /// Read the contents of a register.
    Read {
        /// The register to read.
        reg: String,
        num: Option<u32>,
    },

    /// Modify the contents of a register.
    Write {
        /// The register to write to.
        reg: String,
        val: String,
    },

    /// List the children of the node in the given register path.
    List {
        /// The register to operate on.
        #[clap(default_value = ".")]
        reg: String,
    },

    /// Search for register(s) by name.
    Search {
        /// The string to search for.
        reg: String,
        #[clap(default_value = "10", short, long)]
        max: u32,
    },

    /// Measure the time to read/write registers on each bus.
    Perf {
        /// The number of iterations to perform.
        #[clap(short, default_value = "10000")]
        n: usize,
    },
}

pub struct Tofino {
    map: tofino_regs::RegMap,
    pci: tofino::pci::Pci,
}

impl Tofino {
    pub fn new(dev_path: String) -> Result<Self> {
        let map = tofino_regs::RegMap::new()?;
        let pci = tofino::pci::Pci::new(&dev_path, REGISTER_SIZE)?;
        Ok(Tofino { map, pci })
    }

    // Get the node inside the register tree corresponding to this path
    fn get_node(&self, reg: &str) -> Result<&tofino_regs::Node> {
        self.map.get_node(reg)
    }

    // Get the offset into PCI space that maps this register
    fn get_offset(&self, reg: &str) -> Result<u32> {
        self.map.get_offset(reg)
    }

    // Get all the children of the given node.
    fn get_children(&self, node: &tofino_regs::Node) -> Result<Vec<String>> {
        Ok(tofino_regs::get_children(node))
    }
}

fn search_in(
    ctx: &Tofino,
    cnt: &mut u32,
    max: u32,
    path: &str,
    tgt: &str,
) -> Result<()> {
    let node = ctx
        .get_node(path)
        .with_context(|| format!("Attempting to get node for {path}"))?;
    let children = ctx
        .get_children(node)
        .with_context(|| format!("Attempting to get children of {path}"))?;

    if children.is_empty() {
        if path.contains(tgt) {
            *cnt += 1;
            if *cnt <= max {
                println!("{}", path);
            }
        }
    } else {
        for name in &children {
            let next = format!("{}.{}", path, name);
            search_in(ctx, cnt, max, &next, tgt)?;
        }
    }
    Ok(())
}

pub fn search(ctx: &mut Tofino, max: u32, tgt: String) -> Result<()> {
    let path = String::from(".");

    let mut cnt = 0;
    search_in(ctx, &mut cnt, max, &path, &tgt)?;

    if cnt > max {
        println!("...");
        println!("{} matches found", cnt);
    }

    match cnt {
        0 => Err(anyhow!("not found")),
        _ => Ok(()),
    }
}

fn list(ctx: &Tofino, path: String) -> Result<()> {
    let node = ctx.get_node(&path)?;
    for c in ctx.get_children(node)? {
        if !c.starts_with('_') {
            println!("{}", c);
        }
    }
    Ok(())
}

pub fn read_offset(
    ctx: &mut Tofino,
    mut offset: u32,
    cnt: u32,
) -> Result<Vec<u32>> {
    let mut r = Vec::new();
    for _ in 0..cnt {
        r.push(ctx.pci.read4(offset)?);
        offset += 4;
    }

    Ok(r)
}

pub fn read_register(
    ctx: &mut Tofino,
    reg: &str,
    cnt: u32,
) -> Result<Vec<u32>> {
    let offset = ctx.get_offset(reg)?;
    read_offset(ctx, offset, cnt)
}

fn write_offset(ctx: &mut Tofino, offset: u32, val: u32) -> Result<()> {
    ctx.pci.write4(offset, val)
}

fn cmd_read(ctx: &mut Tofino, reg: &str, cnt: Option<u32>) -> Result<()> {
    let mut cnt = cnt.unwrap_or(1);

    // First try to parse the "reg" as a raw hex offset.
    let mut offset = if let Ok(offset) = parse_val(reg) {
        Ok(offset)

    // Now try as a register name.
    } else if let Ok(node) = ctx.get_node(reg) {
        cnt = node.size / 4;
        Ok(ctx.get_offset(reg)?)
    } else {
        Err(anyhow!("bad register/offset: {}", reg))
    }?;

    let vals = read_offset(ctx, offset, cnt)?;
    for val in vals {
        println!(
            "{}{:x}",
            match cnt > 1 {
                true => format!("{:x}: ", offset),
                false => String::new(),
            },
            val
        );
        offset += 4;
    }
    println!();
    Ok(())
}

// XXX: todo- add support for writing multi-word registers?
// add support for writing bitfields?
fn cmd_write(ctx: &mut Tofino, reg: &str, val: &str) -> Result<()> {
    let offset = if let Ok(offset) = parse_val(reg) {
        Ok(offset)
    } else if let Ok(offset) = ctx.get_offset(reg) {
        Ok(offset)
    } else {
        Err(anyhow!("bad register/offset: {}", reg))
    }?;

    let val = parse_val(val)?;
    write_offset(ctx, offset, val)
}

fn parse_val(v: &str) -> Result<u32> {
    if v.starts_with("0x") {
        let x = v.trim_start_matches("0x");
        u32::from_str_radix(x, 16)
            .map_err(|e| anyhow!("invalid hex word: {:?}", e))
    } else {
        v.parse::<u32>()
            .map_err(|e| anyhow!("invalid value: {:?}", e))
    }
}

fn perf_regs(ctx: &mut Tofino) -> Vec<(String, u32)> {
    let bus_regs = vec![
        ("host", "device_select.pcie_bar01_regs.scratch_reg.0"),
        ("cbus", "device_select.lfltr.0.ctrl.scratch.0"),
        ("mbus", "eth100g_regs.eth100g_reg.scratch.0"),
        ("pbus", "pipes.0.mau.0.dp.mau_scratch"),
    ];

    let mut regs = Vec::new();

    for (bus, reg) in bus_regs {
        if let Ok(a) = ctx.get_offset(reg) {
            regs.push((bus.to_string(), a));
        }
    }
    regs
}

fn pause() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}

fn perf(ctx: &mut Tofino, iter: usize) -> Result<()> {
    println!(
        "{:>5}  {:>8} {:>12} {:>8}    {:>12}  {:>8}",
        "bus", "addr", "read ns", "ns/read", "write ns", "ns/write"
    );

    let regs = perf_regs(ctx);

    for (bus, addr) in &regs {
        pause();
        let start = Utc::now()
            .timestamp_nanos_opt()
            .expect("only a problem after the year 2262");
        for _ in 0..iter {
            let x = ctx.pci.read4(*addr)?;
            if x == 0xffffffff {
                println!("bad read");
            }
        }
        let end = Utc::now()
            .timestamp_nanos_opt()
            .expect("only a problem after the year 2262");
        let read_nsec = end - start;

        pause();
        ctx.pci.write4(*addr, 0)?;
        let start = Utc::now()
            .timestamp_nanos_opt()
            .expect("only a problem after the year 2262");
        for _ in 0..iter {
            ctx.pci.write4(*addr, 0)?;
        }
        let end = Utc::now()
            .timestamp_nanos_opt()
            .expect("only a problem after the year 2262");
        let write_nsec = end - start;
        println!(
            "{:>5}  {:8x} {:>12} {:>8}    {:>12}  {:>8}",
            bus,
            addr,
            read_nsec,
            read_nsec / iter as i64,
            write_nsec,
            write_nsec / iter as i64
        );
    }
    Ok(())
}

fn mac_command(ctx: &mut Tofino, cmd: MacCommands) -> Result<()> {
    match cmd {
        MacCommands::Status { mac } => mac::status(ctx, mac),
    }
}

fn reg_command(ctx: &mut Tofino, cmd: RegCommands) -> Result<()> {
    match cmd {
        RegCommands::Read { reg, num } => cmd_read(ctx, &reg, num),
        RegCommands::Write { reg, val } => cmd_write(ctx, &reg, &val),
        RegCommands::List { reg } => list(ctx, reg),
        RegCommands::Search { max, reg } => search(ctx, max, reg),
        RegCommands::Perf { n } => perf(ctx, n),
    }
}

pub fn exec() -> Result<()> {
    // Parse this first to display help if requested.
    let command = TftoolCommand::parse();

    let dev = match tofino::get_tofino()? {
        Some(node) => node.device_path()?,
        None => bail!("no tofino asic found"),
    };
    let mut ctx = Tofino::new(dev)?;

    match command {
        TftoolCommand::Fuse => fuse::dump_fuse(&mut ctx),
        TftoolCommand::Reg(reg_cmd) => reg_command(&mut ctx, reg_cmd),
        TftoolCommand::Mac(mac_cmd) => mac_command(&mut ctx, mac_cmd),
        TftoolCommand::Dr(dr_cmd) => dr::dr_command(&mut ctx, dr_cmd),
    }
}
