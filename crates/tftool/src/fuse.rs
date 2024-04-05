// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use anyhow::Result;

use crate::Tofino;
use tofino::fuse;

macro_rules! print_field {
    ($struct:ident, $field:ident) => {
        println!("{:24}: 0x{:x}", stringify!($field), $struct.$field)
    };
}

pub fn dump_fuse(ctx: &mut Tofino) -> Result<()> {
    let fuse = fuse::Fuse::read(&ctx.pci)?;

    print_field!(fuse, device_id);
    print_field!(fuse, version);
    print_field!(fuse, freq_dis);
    print_field!(fuse, freq_bps);
    print_field!(fuse, freq_pps);
    print_field!(fuse, pcie_dis);
    print_field!(fuse, cpu_speed_dis);
    print_field!(fuse, speed_dis);
    print_field!(fuse, port_dis);
    print_field!(fuse, pipe_dis);
    print_field!(fuse, pipe0_mau_dis);
    print_field!(fuse, pipe1_mau_dis);
    print_field!(fuse, pipe2_mau_dis);
    print_field!(fuse, pipe3_mau_dis);
    print_field!(fuse, tm_mem_dis);
    print_field!(fuse, bsync_dis);
    print_field!(fuse, pgen_dis);
    print_field!(fuse, resub_dis);
    print_field!(fuse, voltage_scaling);
    print_field!(fuse, rsvd_22);
    print_field!(fuse, part_num);
    print_field!(fuse, rev_num);
    print_field!(fuse, pkg_id);
    print_field!(fuse, silent_spin);
    print_field!(fuse, chip_id);
    print_field!(fuse, pmro_and_skew);
    print_field!(fuse, wf_core_repair);
    print_field!(fuse, core_repair);
    print_field!(fuse, tile_repair);
    print_field!(fuse, freq_bps_2);
    print_field!(fuse, freq_pps_2);
    print_field!(fuse, die_rotation);
    print_field!(fuse, soft_pipe_dis);

    let chip_id: fuse::ChipId = fuse.chip_id.into();
    println!("{:24}: {}", "wafer id", chip_id);
    Ok(())
}
