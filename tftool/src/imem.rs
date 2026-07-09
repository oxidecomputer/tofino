// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2026 Oxide Computer Company

//! Read MAU instruction memory (imem) by PHV container.
//!
//! Every PHV container has a VLIW ALU in each MAU stage, and each ALU has 32
//! instruction-memory words (one per instruction line, with a color bit that
//! selects between the two action-instruction addresses sharing a line; line
//! 31 color 1 is the always-run action slot). This module maps a container
//! name like `W3` or `MH6` to its ALU's imem words in a given pipe/stage and
//! reads them over the register interface.
//!
//! The container-to-imem mapping mirrors bf-asm: PHV container uids are
//! allocated as W(32b) x4 groups, B(8b) x4 groups, H(16b) x6 groups, each
//! group holding 12 normal + 4 mocha + 4 dark containers (jbay/phv.cpp), and
//! the imem arrays are indexed [side][group][alu][line] where W/B split their
//! 4 groups 2 per side and H splits its 6 groups 3 per side
//! (jbay/instruction.cpp).

use anyhow::{Result, anyhow, bail};
use paste::paste;
use rust_rpi::RegisterInstance;

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Class {
    /// 32-bit containers (W)
    Word,
    /// 16-bit containers (H)
    Half,
    /// 8-bit containers (B)
    Byte,
}

impl Class {
    fn letter(&self) -> char {
        match self {
            Class::Word => 'W',
            Class::Half => 'H',
            Class::Byte => 'B',
        }
    }

    fn bits(&self) -> u32 {
        match self {
            Class::Word => 32,
            Class::Half => 16,
            Class::Byte => 8,
        }
    }

    /// Number of container groups of this class per imem array side.
    fn groups_per_side(&self) -> u32 {
        match self {
            Class::Half => 3,
            _ => 2,
        }
    }

    /// The group dimension of the imem arrays (H arrays are padded to 4).
    fn group_dim(&self) -> u32 {
        match self {
            Class::Half => 4,
            _ => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Normal,
    Mocha,
    Dark,
}

/// The ALU (imem location) attached to one PHV container.
#[derive(Debug)]
struct PhvAlu {
    name: String,
    class: Class,
    kind: Kind,
    side: u32,
    group: u32,
    alu: u32,
}

impl PhvAlu {
    fn parse(name: &str) -> Result<Self> {
        let upper = name.to_uppercase();
        let mut chars = upper.chars();
        let mut c = chars.next().ok_or_else(|| anyhow!("empty phv name"))?;
        let kind = match c {
            'M' => {
                c = chars.next().ok_or_else(|| anyhow!("bad phv name"))?;
                Kind::Mocha
            }
            'D' => {
                c = chars.next().ok_or_else(|| anyhow!("bad phv name"))?;
                Kind::Dark
            }
            _ => Kind::Normal,
        };
        let class = match c {
            'W' => Class::Word,
            'H' => Class::Half,
            'B' => Class::Byte,
            _ => bail!("bad phv class '{c}': expected W, H or B"),
        };
        let number: u32 = chars
            .as_str()
            .parse()
            .map_err(|_| anyhow!("bad phv container number"))?;

        // Each group holds 12 normal and 4 mocha / 4 dark containers.
        let per_group = match kind {
            Kind::Normal => 12,
            _ => 4,
        };
        let group_count = 2 * class.groups_per_side();
        if number >= per_group * group_count {
            bail!("{upper} out of range");
        }
        let phv_group = number / per_group;
        let alu = number % per_group;

        Ok(PhvAlu {
            name: upper.clone(),
            class,
            kind,
            side: phv_group / class.groups_per_side(),
            group: phv_group % class.groups_per_side(),
            alu,
        })
    }

    /// Flat index into the (linearized) imem register array for one
    /// instruction line. The arrays are [side][group][alu][line] with the
    /// alu dimension padded to 16 for normal containers and 4 for
    /// mocha/dark.
    fn index(&self, line: u32) -> u32 {
        let alu_dim = match self.kind {
            Kind::Normal => 16,
            _ => 4,
        };
        (((self.side * self.class.group_dim() + self.group) * alu_dim
            + self.alu)
            * 32)
            + line
    }

    /// imem word layout: instruction bits [instr_bits-1:0], then a color bit,
    /// then a parity bit.
    fn instr_bits(&self) -> u32 {
        match (self.kind, self.class) {
            (Kind::Normal, Class::Word) => 27,
            (Kind::Normal, Class::Half) => 24,
            (Kind::Normal, Class::Byte) => 21,
            (Kind::Mocha, _) => 7,
            (Kind::Dark, _) => 6,
        }
    }

    /// Name the container at ALU slot `slot` (0..19) of this ALU's group:
    /// slots 0-11 are normal, 12-15 mocha, 16-19 dark.
    fn slot_container(&self, slot: u32) -> String {
        let letter = self.class.letter();
        let group = self.side * self.class.groups_per_side() + self.group;
        match slot {
            0..=11 => format!("{letter}{}", group * 12 + slot),
            12..=15 => format!("M{letter}{}", group * 4 + slot - 12),
            16..=19 => format!("D{letter}{}", group * 4 + slot - 16),
            _ => format!("{letter}?slot{slot}"),
        }
    }

    /// Decode a 6-bit VLIW source operand (bf-asm VLIW::Operand encoding):
    /// 0x20|n selects action data bus entry n, 20..31 is a small constant
    /// (value+24), 0..19 is a PHV slot within this ALU's group.
    fn decode_src(&self, src: u32) -> String {
        if src & 0x20 != 0 {
            format!("adb[{}]", src & 0x1f)
        } else if src >= 20 {
            format!("const {}", src as i32 - 24)
        } else {
            self.slot_container(src)
        }
    }

    /// Decode one instruction word. Mirrors the encoders in bf-asm
    /// instruction.cpp (DepositField::encode, Set::encode) with
    /// INSTR_SRC2_BITS=5; ops other than deposit-field/set are shown raw.
    fn decode_instr(&self, instr: u32) -> String {
        if instr == 0 {
            return String::new();
        }
        match self.kind {
            // mocha: set only; instr = src | 0x40
            Kind::Mocha => {
                if instr & 0x40 != 0 {
                    format!(
                        "set {}, {}",
                        self.name,
                        self.decode_src(instr & 0x3f)
                    )
                } else {
                    format!("mocha op {instr:#x}")
                }
            }
            // dark: set only; instr = phv slot | 0x20
            Kind::Dark => {
                if instr & 0x20 != 0 && instr & !0x3f == 0 {
                    format!(
                        "set {}, {}",
                        self.name,
                        self.slot_container(instr & 0x1f)
                    )
                } else {
                    format!("dark op {instr:#x}")
                }
            }
            Kind::Normal => {
                let size = self.class.bits();
                let src2 = instr & 0x1f;
                let upper = instr >> 5;
                if upper & 0x40 != 0 {
                    // deposit-field: marker bit 6, dest.hi<<7, rot<<12, and
                    // container-size-dependent dest.lo packing
                    let src1 = upper & 0x3f;
                    let hi = (upper >> 7) & 0x1f;
                    let (lo, rot) = match self.class {
                        Class::Word => {
                            ((upper >> 17) & 0x1f, (upper >> 12) & 0x1f)
                        }
                        Class::Half => (
                            ((upper >> 11) & 1) | (((upper >> 16) & 0x7) << 1),
                            (upper >> 12) & 0xf,
                        ),
                        Class::Byte => (
                            ((upper >> 10) & 3) | (((upper >> 15) & 1) << 2),
                            (upper >> 12) & 0x7,
                        ),
                    };
                    // small constants fold the barrel rotate into the
                    // encoding; recover the effective value
                    let src_txt = if src1 & 0x20 == 0 && src1 >= 20 {
                        let mask = u32::MAX >> (32 - size);
                        let val = (src1 as i64 - 24) as u32 & mask;
                        let eff = val.rotate_right((rot + lo) % size) & mask;
                        format!("{eff}")
                    } else {
                        format!("{} >>rot {rot}", self.decode_src(src1))
                    };
                    let bg = match src2 {
                        0 => String::new(),
                        s => format!(" (bg {})", self.slot_container(s)),
                    };
                    format!(
                        "deposit-field {}({lo}..{hi}), {src_txt}{bg}",
                        self.name
                    )
                } else {
                    let opcode = upper >> 6;
                    let src1 = upper & 0x3f;
                    match opcode {
                        // opA ("A" = pass src1), used for full-container set
                        0x31e => {
                            format!(
                                "set {}, {}",
                                self.name,
                                self.decode_src(src1)
                            )
                        }
                        _ => format!(
                            "op {opcode:#x} src1={} src2={}",
                            self.decode_src(src1),
                            self.slot_container(src2)
                        ),
                    }
                }
            }
        }
    }
}

macro_rules! read_imem_word {
    ($name:ident) => {
        paste! {
        fn [<read_ $name>](
            ctx: &mut Tofino,
            pipe: u32,
            stage: u32,
            index: u32,
        ) -> Result<(u32, u32)> {
            let word = regs::Client::default()
                .pipes(pipe)?
                .mau(stage)?
                .dp()
                .imem()
                .$name(index)?;
            let value = u32::from(word.read(ctx)?);
            Ok((word.addr, value))
        }
        }
    };
}

read_imem_word!(imem_subword_32);
read_imem_word!(imem_subword_16);
read_imem_word!(imem_subword_8);
read_imem_word!(imem_mocha_subword_32);
read_imem_word!(imem_mocha_subword_16);
read_imem_word!(imem_mocha_subword_8);
read_imem_word!(imem_dark_subword_32);
read_imem_word!(imem_dark_subword_16);
read_imem_word!(imem_dark_subword_8);

fn read_word(
    ctx: &mut Tofino,
    alu: &PhvAlu,
    pipe: u32,
    stage: u32,
    index: u32,
) -> Result<(u32, u32)> {
    match (alu.kind, alu.class) {
        (Kind::Normal, Class::Word) => {
            read_imem_subword_32(ctx, pipe, stage, index)
        }
        (Kind::Normal, Class::Half) => {
            read_imem_subword_16(ctx, pipe, stage, index)
        }
        (Kind::Normal, Class::Byte) => {
            read_imem_subword_8(ctx, pipe, stage, index)
        }
        (Kind::Mocha, Class::Word) => {
            read_imem_mocha_subword_32(ctx, pipe, stage, index)
        }
        (Kind::Mocha, Class::Half) => {
            read_imem_mocha_subword_16(ctx, pipe, stage, index)
        }
        (Kind::Mocha, Class::Byte) => {
            read_imem_mocha_subword_8(ctx, pipe, stage, index)
        }
        (Kind::Dark, Class::Word) => {
            read_imem_dark_subword_32(ctx, pipe, stage, index)
        }
        (Kind::Dark, Class::Half) => {
            read_imem_dark_subword_16(ctx, pipe, stage, index)
        }
        (Kind::Dark, Class::Byte) => {
            read_imem_dark_subword_8(ctx, pipe, stage, index)
        }
    }
}

fn read(ctx: &mut Tofino, phv: &str, pipe: u32, stage: u32) -> Result<()> {
    if pipe > 3 {
        bail!("pipe must be 0-3");
    }
    if stage > 19 {
        bail!("stage must be 0-19");
    }
    let alu = PhvAlu::parse(phv)?;

    println!(
        "{} imem in pipe {pipe} stage {stage} \
         (side {} group {} alu {}):",
        alu.name, alu.side, alu.group, alu.alu,
    );
    println!(
        "{:>4} {:>8} {:>8} {:>7} {:>1} {:>1}  DECODE",
        "LINE", "ADDR", "RAW", "INSTR", "C", "P"
    );

    let mut zero_run: Option<(u32, u32)> = None;
    let flush = |run: &mut Option<(u32, u32)>| {
        if let Some((first, last)) = run.take() {
            if first == last {
                println!("{first:>4} (zero)");
            } else {
                println!("{:>4} lines {first}..{last} zero", "");
            }
        }
    };

    for line in 0..32 {
        let (addr, value) = read_word(ctx, &alu, pipe, stage, alu.index(line))?;
        if value == 0 {
            zero_run = match zero_run {
                None => Some((line, line)),
                Some((first, _)) => Some((first, line)),
            };
            continue;
        }
        flush(&mut zero_run);
        let instr = value & (u32::MAX >> (32 - alu.instr_bits()));
        let color = (value >> alu.instr_bits()) & 1;
        let parity = (value >> (alu.instr_bits() + 1)) & 1;
        let ara = match (line, color) {
            (31, 1) => " [always-run line]",
            _ => "",
        };
        println!(
            "{line:>4} {addr:08x} {value:08x} {instr:>7x} {color} {parity}  {}{ara}",
            alu.decode_instr(instr),
        );
    }
    flush(&mut zero_run);
    Ok(())
}

pub fn imem_command(ctx: &mut Tofino, cmd: ImemCommands) -> Result<()> {
    match cmd {
        ImemCommands::Read { phv, pipe, stage } => read(ctx, &phv, pipe, stage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr_of(alu: &PhvAlu, pipe: u32, stage: u32, line: u32) -> u32 {
        let imem = regs::Client::default()
            .pipes(pipe)
            .unwrap()
            .mau(stage)
            .unwrap()
            .dp()
            .imem();
        let index = alu.index(line);
        match (alu.kind, alu.class) {
            (Kind::Normal, Class::Word) => {
                imem.imem_subword_32(index).unwrap().addr
            }
            (Kind::Normal, Class::Half) => {
                imem.imem_subword_16(index).unwrap().addr
            }
            (Kind::Mocha, Class::Half) => {
                imem.imem_mocha_subword_16(index).unwrap().addr
            }
            (Kind::Dark, Class::Word) => {
                imem.imem_dark_subword_32(index).unwrap().addr
            }
            _ => unimplemented!(),
        }
    }

    // Expected addresses cross-checked against the sidecar tofino2.bin
    // config blob (pipe-0 imem block writes) and bf-asm's layout.
    #[test]
    fn w3_stage13_addresses() {
        let alu = PhvAlu::parse("W3").unwrap();
        assert_eq!((alu.side, alu.group, alu.alu), (0, 0, 3));
        // pipe 2 base 0x06000000, stage 13 base +0x680000, subword32 @
        // +0xc000, W3 words at flat index 96
        assert_eq!(addr_of(&alu, 2, 13, 0), 0x0668c180);
        assert_eq!(addr_of(&alu, 2, 13, 31), 0x0668c1fc);
    }

    #[test]
    fn known_sidecar_words() {
        // sidecar stage 13: set MH6, $data0 lives at pipe0 +0x300
        let mh6 = PhvAlu::parse("MH6").unwrap();
        assert_eq!((mh6.side, mh6.group, mh6.alu), (0, 1, 2));
        assert_eq!(addr_of(&mh6, 0, 13, 0), 0x04680300);

        // sidecar stage 13: add H20, MH6, H20 at line 1 -> pipe0 +0x8c04
        let h20 = PhvAlu::parse("H20").unwrap();
        assert_eq!((h20.side, h20.group, h20.alu), (0, 1, 8));
        assert_eq!(addr_of(&h20, 0, 13, 1), 0x04688c04);

        // sidecar stage 12 always-run: set DW8, W24 at line 31
        let dw8 = PhvAlu::parse("DW8").unwrap();
        assert_eq!((dw8.side, dw8.group, dw8.alu), (1, 0, 0));
        assert_eq!(addr_of(&dw8, 0, 12, 31), 0x0460347c);
    }

    #[test]
    fn decode_known_instructions() {
        // stage 13 sidecar1420: set W21(24..31), 16
        let w21 = PhvAlu::parse("W21").unwrap();
        assert_eq!(
            w21.decode_instr(0x609fb29),
            "deposit-field W21(24..31), 16 (bg W21)"
        );
        // stage 12 always-run: set W24, 0 / set W36, 0
        let w24 = PhvAlu::parse("W24").unwrap();
        assert_eq!(w24.decode_instr(0x18f308), "set W24, const 0");
        // stage 12 always-run: set DW8, W24
        let dw8 = PhvAlu::parse("DW8").unwrap();
        assert_eq!(dw8.decode_instr(0x20), "set DW8, W24");
        // stage 13 select_route: set MH6, adb[0]
        let mh6 = PhvAlu::parse("MH6").unwrap();
        assert_eq!(mh6.decode_instr(0x60), "set MH6, adb[0]");
        // the hypothesized foreign word: deposit-field W3(16..27), 0
        let w3 = PhvAlu::parse("W3").unwrap();
        assert_eq!(
            w3.decode_instr(0x421bb03),
            "deposit-field W3(16..27), 0 (bg W3)"
        );
    }
}
