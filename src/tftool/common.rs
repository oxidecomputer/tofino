// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

pub fn get_bit(word: impl std::convert::Into<u32>, bit: usize) -> u64 {
    let w: u32 = word.into();
    ((w >> bit) & 0x1) as u64
}

pub fn get_bits(regs: &[u32], start: u8, end: u8) -> u64 {
    let mut rval = 0u64;

    let start = start as isize;
    let end = end as isize;
    for idx in (start..end + 1).rev() {
        let word = (idx / 32) as usize;
        let bit = (idx % 32) as usize;
        rval = (rval << 1) | get_bit(regs[word], bit);
        if idx == 0 {
            break;
        }
    }
    rval
}

#[test]
fn test_get_bits() {
    assert_eq!(get_bits(&[0xabcd], 0, 3), 0xd);
    assert_eq!(get_bits(&[0xabcd], 4, 7), 0xc);
    assert_eq!(get_bits(&[0xabcd], 8, 11), 0xb);
    assert_eq!(get_bits(&[0xabcd], 12, 15), 0xa);
}
