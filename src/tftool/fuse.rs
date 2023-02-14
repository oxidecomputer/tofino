// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use anyhow::{anyhow, Result};

use crate::common::get_bits;
use crate::Tofino;

fn get_byte(full: u64, start: u8, end: u8) -> u8 {
    let mask = (1u64 << (end - start)) - 1;
    ((full >> start) & mask) as u8
}

fn chip_id_to_wafer(chip_id: u64) -> String {
    let chip_id = chip_id.reverse_bits();
    let fab = get_byte(chip_id, 57, 64) as char;
    let lot = get_byte(chip_id, 50, 57) as char;
    let lotnum0 = get_byte(chip_id, 43, 50) as char;
    let lotnum1 = get_byte(chip_id, 36, 43) as char;
    let lotnum2 = get_byte(chip_id, 29, 36) as char;
    let lotnum3 = get_byte(chip_id, 22, 29) as char;
    let wafer = get_byte(chip_id, 17, 22);
    let x = get_byte(chip_id, 9, 17);
    let y = get_byte(chip_id, 1, 9);

    format!(
        "{}{}{}{}{}{}-W{}-X{}-Y{}",
        fab, lot, lotnum0, lotnum1, lotnum2, lotnum3, wafer, x, y,
    )
}

fn print_field(r: &[u32], name: &str, low: u8, high: u8) {
    println!("{:24}: 0x{:x}", name, get_bits(r, low, high));
}

#[cfg(feature = "tofino_regs")]
fn read_fuse(ctx: &mut Tofino) -> Result<Vec<u32>> {
    const FUSE_NAME: &str = "device_select.misc_regs.func_fuse";
    crate::read_register(ctx, FUSE_NAME, 8)
}

#[cfg(not(feature = "tofino_regs"))]
fn read_fuse(ctx: &mut Tofino) -> Result<Vec<u32>> {
    crate::read_offset(ctx, 0x80180, 8)
}

pub fn dump_fuse(ctx: &mut Tofino) -> Result<()> {
    let r = read_fuse(ctx)?;
    if r.len() != 8 {
        return Err(anyhow!("fuse should be 8 words.  Found {}", r.len()));
    }

    print_field(&r, "resubmit_disable", 1, 1);
    print_field(&r, "mau_tcam_reduction", 2, 2);
    print_field(&r, "mau_sram_reduction", 3, 3);
    print_field(&r, "packet_generator_disable", 4, 4);
    print_field(&r, "pipe_disable", 5, 8);
    print_field(&r, "mau_stage_disable", 9, 20);
    print_field(&r, "port_disable_map_lo", 21, 84);
    print_field(&r, "port_disable_map_hi", 85, 85);
    print_field(&r, "tm_memory_disable", 86, 121);
    print_field(&r, "port_speed_reduction", 126, 127);
    print_field(&r, "cpu_port_speed_reduction", 128, 129);
    print_field(&r, "pcie_lane_reduction", 130, 131);
    print_field(&r, "baresync_disable", 132, 132);
    print_field(&r, "frequency_reduction", 133, 134);
    print_field(&r, "frequency_check_disable", 135, 135);
    print_field(&r, "versioning", 139, 140);
    print_field(&r, "chip_part_number", 151, 155);
    print_field(&r, "part_revision_number", 156, 163);
    print_field(&r, "package_id", 164, 165);
    print_field(&r, "silent_spin", 166, 167);
    print_field(&r, "pmro_and_skew", 231, 242);
    print_field(&r, "voltage_scaling", 243, 245);
    print_field(&r, "chip_id", 168, 230);

    let chip_id = get_bits(&r, 168, 230);
    println!("{:24}: {}", "wafer id", chip_id_to_wafer(chip_id));
    Ok(())
}
