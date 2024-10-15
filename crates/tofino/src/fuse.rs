// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use std::convert::From;
use std::fmt;

use anyhow::{anyhow, Result};

use crate::common::get_bits;
use crate::pci::Pci;

/// Offset of the first register holding fuse data
const FUSE_OFFSET: u32 = 0x80180;

/// Number of 4-byte words of fuse data
const FUSE_SIZE: u32 = 16;

/// Data stored in the Fuse registers in the Tofino ASIC
pub struct Fuse {
    pub device_id: u64,       // 16 bits
    pub version: u64,         // 2 bits
    pub freq_dis: u64,        // 1 bit
    pub freq_bps: u64,        // 2 bits
    pub freq_pps: u64,        // 2 bits
    pub pcie_dis: u64,        // 2 bits
    pub cpu_speed_dis: u64,   // 2 bits
    pub speed_dis: u64,       // 64 bits
    pub port_dis: u64,        // 40 bits
    pub pipe_dis: u64,        // 4 bits
    pub pipe0_mau_dis: u64,   // 21 bits
    pub pipe1_mau_dis: u64,   // 21 bits
    pub pipe2_mau_dis: u64,   // 21 bits
    pub pipe3_mau_dis: u64,   // 21 bits
    pub tm_mem_dis: u64,      // 32 bits
    pub bsync_dis: u64,       // 1 bit
    pub pgen_dis: u64,        // 1 bit
    pub resub_dis: u64,       // 1 bit
    pub voltage_scaling: u64, // 12 bits
    pub rsvd_22: u64,         // 22 bits
    pub part_num: u64,        // 13 bits
    pub rev_num: u64,         // 8 bits
    pub pkg_id: u64,          // 2 bits
    pub silent_spin: u64,     // 2 bits
    pub chip_id: u64,         // 63 bits
    pub pmro_and_skew: u64,   // 12 bits
    pub wf_core_repair: u64,  // 1 bit
    pub core_repair: u64,     // 1 bit
    pub tile_repair: u64,     // 1 bit
    pub freq_bps_2: u64,      // 4 bits
    pub freq_pps_2: u64,      // 4 bits
    pub die_rotation: u64,    // 1 bit
    pub soft_pipe_dis: u64,   // 4 bits
}

impl Fuse {
    pub fn try_from_slice(data: &[u32]) -> Result<Fuse> {
        if data.len() != FUSE_SIZE as usize {
            return Err(anyhow!(
                "fuse should be {FUSE_SIZE} words.  Found {}",
                data.len()
            ));
        }

        Ok(Fuse {
            device_id: get_bits(data, 0, 15),
            version: get_bits(data, 16, 17),
            freq_dis: get_bits(data, 18, 18),
            freq_bps: get_bits(data, 19, 20),
            freq_pps: get_bits(data, 21, 22),
            pcie_dis: get_bits(data, 23, 24),
            cpu_speed_dis: get_bits(data, 25, 26),
            speed_dis: get_bits(data, 27, 90),
            port_dis: get_bits(data, 91, 130),
            pipe_dis: get_bits(data, 131, 134),
            pipe0_mau_dis: get_bits(data, 135, 155),
            pipe1_mau_dis: get_bits(data, 156, 176),
            pipe2_mau_dis: get_bits(data, 177, 197),
            pipe3_mau_dis: get_bits(data, 198, 218),
            tm_mem_dis: get_bits(data, 219, 250),
            bsync_dis: get_bits(data, 251, 251),
            pgen_dis: get_bits(data, 252, 252),
            resub_dis: get_bits(data, 253, 253),
            voltage_scaling: get_bits(data, 254, 265),
            rsvd_22: get_bits(data, 266, 287),
            part_num: get_bits(data, 288, 301),
            rev_num: get_bits(data, 302, 309),
            pkg_id: get_bits(data, 310, 311),
            silent_spin: get_bits(data, 312, 313),
            chip_id: get_bits(data, 314, 376),
            pmro_and_skew: get_bits(data, 377, 388),
            wf_core_repair: get_bits(data, 389, 389),
            core_repair: get_bits(data, 390, 390),
            tile_repair: get_bits(data, 391, 391),
            freq_bps_2: get_bits(data, 392, 395),
            freq_pps_2: get_bits(data, 396, 399),
            die_rotation: get_bits(data, 400, 400),
            soft_pipe_dis: get_bits(data, 401, 404),
        })
    }

    pub fn read(pci: &Pci) -> Result<Self> {
        Self::try_from_slice(&read_raw(pci)?)
    }
}

/// Parsed version of the chip_id field in the Fuse struct
pub struct ChipId {
    pub fab: char,
    pub lot: char,
    pub lotnum0: char,
    pub lotnum1: char,
    pub lotnum2: char,
    pub lotnum3: char,
    pub wafer: u8,
    pub xsign: u8,
    pub x: u8,
    pub ysign: u8,
    pub y: u8,
}

impl From<u64> for ChipId {
    fn from(mut raw: u64) -> Self {
        fn bite(a: &mut u64, bits: usize) -> u8 {
            let rval = (*a & ((1u64 << bits) - 1)) as u8;
            *a >>= bits;
            rval
        }

        ChipId {
            fab: bite(&mut raw, 7) as char,
            lot: bite(&mut raw, 7) as char,
            lotnum0: bite(&mut raw, 7) as char,
            lotnum1: bite(&mut raw, 7) as char,
            lotnum2: bite(&mut raw, 7) as char,
            lotnum3: bite(&mut raw, 7) as char,
            wafer: bite(&mut raw, 5),
            xsign: bite(&mut raw, 1),
            x: bite(&mut raw, 7),
            ysign: bite(&mut raw, 1),
            y: bite(&mut raw, 7),
        }
    }
}

impl fmt::Display for ChipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}{}{} Wafer {} X={}{} Y={}{}",
            self.fab,
            self.lot,
            self.lotnum0,
            self.lotnum1,
            self.lotnum2,
            self.lotnum3,
            self.wafer,
            match self.xsign {
                0 => '+',
                _ => '-',
            },
            self.x,
            match self.ysign {
                0 => '+',
                _ => '-',
            },
            self.y,
        )
    }
}

pub fn read_raw(pci: &Pci) -> Result<Vec<u32>> {
    let mut r = Vec::with_capacity(FUSE_SIZE as usize);
    let mut offset = FUSE_OFFSET;
    for _ in 0..FUSE_SIZE {
        r.push(pci.read4(offset)?);
        offset += 4;
    }

    Ok(r)
}

#[test]
fn test_chip() {
    let c = ChipId::from(0x08025dbb797061d4u64);
    assert_eq!(c.fab, 'T');
    assert_eq!(c.lot, 'C');
    assert_eq!(c.lotnum0, 'A');
    assert_eq!(c.lotnum1, 'K');
    assert_eq!(c.lotnum2, '7');
    assert_eq!(c.lotnum3, '7');
    assert_eq!(c.wafer, 23);
    assert_eq!(c.xsign, 0);
    assert_eq!(c.x, 2);
    assert_eq!(c.ysign, 0);
    assert_eq!(c.y, 8);
}
