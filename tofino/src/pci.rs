// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

/// This provides a small number of utility routines for accessing the
/// ASIC's memory mapped PCI space.
use std::ffi::CStr;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;

use anyhow::{Result, anyhow};
use nix::poll;

unsafe extern "C" {
    pub fn pci_open(path: *const ::std::os::raw::c_char) -> ::core::ffi::c_int;
    pub fn pci_close(fd: ::core::ffi::c_int);
    pub fn pci_map(
        fd: ::core::ffi::c_int,
        size: usize,
    ) -> *mut ::core::ffi::c_void;
    pub fn pci_unmap(base: *mut ::core::ffi::c_void, size: usize);
    pub fn pci_check_presence(
        path: *const ::std::os::raw::c_char,
    ) -> ::core::ffi::c_int;
    pub fn pci_err_msg() -> *const ::std::os::raw::c_char;
}

/// A handle representing a mapped ASIC device
pub struct Pci {
    dev_file: File,
    ptr: *mut ::core::ffi::c_void,
    len: usize,
}

impl Drop for Pci {
    fn drop(&mut self) {
        unsafe {
            pci_unmap(self.ptr, self.len);
        };
    }
}

fn open_device_file(path: &str) -> std::io::Result<File> {
    std::fs::OpenOptions::new().read(true).write(true).open(path)
}

impl Pci {
    /// Open the ASIC and map the BAR containing the config/status registers.
    pub fn new(path: &str, len: usize) -> Result<Self> {
        let dev_file = open_device_file(path)?;
        let ptr = unsafe { pci_map(dev_file.as_raw_fd(), len) };

        if ptr.is_null() {
            let msg = unsafe {
                CStr::from_ptr(pci_err_msg()).to_string_lossy().into_owned()
            };
            Err(anyhow!("failed to map {}: {}", path, msg))
        } else {
            Ok(Pci { dev_file, ptr, len })
        }
    }

    /// Block until the underlying device file becomes "readable".  For this
    /// driver, this means that one or more interrupts have been caught, and
    /// the exported shadow interrupt bitmap has been updated.
    pub fn poll(&self, timeout: std::time::Duration) -> Result<bool> {
        let fd = self.dev_file.as_fd();
        let mut pollfds = [poll::PollFd::new(fd, poll::PollFlags::POLLRDNORM)];

        let timeout = poll::PollTimeout::try_from(timeout).map_err(|e| {
            anyhow::anyhow!(format!("invalid timeout {timeout:?}: {e:?}"))
        })?;

        poll::poll(&mut pollfds, timeout)
            .map_err(|e| anyhow::anyhow!("poll failed: {e:?}"))
            .map(|nready| nready > 0)
    }

    /// Fetches the set of shadow interrupt bits, indicating which interrupts
    ///  have fired since the last time this process issued this read.
    pub fn read_shadow_bits(&mut self) -> Result<Vec<u8>> {
        let mut buffer = [0u8; 16];
        self.dev_file
            .read(&mut buffer)
            .map_err(|e| anyhow!(format!("failed to read shadow bits: {e:?}")))
            .map(|r| buffer[0..r].to_vec())
    }

    /// Attempt to open the device file, just to determine whether the
    /// underlying ASIC is still available.
    pub fn check_presence(path: &str) -> bool {
        open_device_file(path).is_ok()
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
