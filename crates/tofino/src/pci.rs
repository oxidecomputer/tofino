// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

/// This provides a small number of utility routines for accessing the
/// ASIC's memory mapped PCI space.
use anyhow::{anyhow, Result};
use std::ffi::{CStr, CString};

extern "C" {
    pub fn pci_map(
        path: *const ::std::os::raw::c_char,
        size: usize,
    ) -> *mut ::core::ffi::c_void;
    pub fn pci_err_msg() -> *const ::std::os::raw::c_char;
}

/// A handle representing a mapped ASIC device
pub struct Pci {
    ptr: *mut ::core::ffi::c_void,
    len: usize,
}

impl Pci {
    /// Open the ASIC and map the BAR containing the config/status registers.
    pub fn new(path: &str, len: usize) -> Result<Self> {
        let ptr = unsafe {
            let path = CString::new(path).unwrap();
            pci_map(path.as_ptr(), len)
        };

        if ptr.is_null() {
            let msg = unsafe {
                CStr::from_ptr(pci_err_msg()).to_string_lossy().into_owned()
            };
            Err(anyhow!("failed to map {}: {}", path, msg))
        } else {
            Ok(Pci { ptr, len })
        }
    }

    fn get_word_ptr(&self, offset: u32) -> Result<*mut u32> {
        if offset & 0x3 != 0 {
            Err(anyhow!("unaligned 4-byte read at {}", offset))
        } else if offset + 4 >= self.len as u32 {
            Err(anyhow!("offset {} is outside the mapped range", offset))
        } else {
            let ptr =
                unsafe { (self.ptr as *mut u32).add(offset as usize >> 2) };
            Ok(ptr)
        }
    }

    /// Read a 4-byte word from the given offset
    pub fn read4(&self, offset: u32) -> Result<u32> {
        let ptr = self.get_word_ptr(offset)?;
        unsafe { Ok(std::ptr::read(ptr)) }
    }

    /// Write a 4-byte word to the given offset
    pub fn write4(&self, offset: u32, val: u32) -> Result<()> {
        let ptr = self.get_word_ptr(offset)?;
        unsafe {
            std::ptr::write(ptr, val);
        }
        Ok(())
    }
}
