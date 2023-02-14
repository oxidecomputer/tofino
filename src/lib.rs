// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use anyhow::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct TofinoNode {
    pub name: String,
    pub driver: Option<String>,
    pub instance: Option<i32>,
    pub devfs_path: String,
}

impl TofinoNode {
    pub fn device_path(&self) -> Result<String, Error> {
        plat::device_path(self)
    }

    pub fn has_driver(&self) -> bool {
        self.driver.is_some()
    }

    pub fn has_asic(&self) -> bool {
        self.instance.is_some()
    }
}

#[cfg(target_os = "illumos")]
mod plat {
    use std::path::PathBuf;

    use anyhow::{anyhow, bail, Context, Error, Result};
    use illumos_devinfo::DevInfo;

    const TOFINO_SUBSYSTEM_VID: i32 = 0x1d1c;
    const TOFINO_SUBSYSTEM_ID: [i32; 5] = [
        0x0001, // TF1_A0
        0x0010, // TF1_B0
        0x0100, // TF2_A0
        0x0000, // TF2_A00
        0x0110, // TF2_B0
    ];

    // Given a node name from the devinfo snapshot, determine whether it
    // represents one of the tofino models we support.
    fn is_tofino_node_name(name: &str) -> bool {
        if let Some(pci) = name.strip_prefix("pci") {
            if let Some((vid, id)) = pci.split_once(',') {
                let vid = match i32::from_str_radix(vid, 16) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let id = match i32::from_str_radix(id, 16) {
                    Ok(i) => i,
                    Err(_) => return false,
                };

                return vid == TOFINO_SUBSYSTEM_VID
                    && TOFINO_SUBSYSTEM_ID.iter().any(|&x| x == id);
            }
        }

        false
    }

    // Load the devinfo map, and scan it for any node representing a tofino asic.
    // TODO-completeness: We could return a vector containing all the asics.  Since
    // we know that our current platform can have no more than one, we stop scanning
    // as soon as we find that one.
    pub fn get_tofino_nodes() -> Result<Vec<crate::TofinoNode>> {
        let mut device_info =
            DevInfo::new_force_load().with_context(|| "loading devinfo map")?;
        get_tofino_nodes_from(&mut device_info)
    }

    pub fn get_tofino_nodes_from(
        device_info: &mut DevInfo,
    ) -> Result<Vec<crate::TofinoNode>> {
        let mut node_walker = device_info.walk_node();
        while let Some(node) = node_walker
            .next()
            .transpose()
            .map_err(|e| anyhow!("unable to walk device tree: {:?}", e))?
        {
            if is_tofino_node_name(&node.node_name()) {
                return Ok(vec![crate::TofinoNode {
                    name: node.node_name(),
                    driver: node.driver_name(),
                    instance: node.instance(),
                    devfs_path: node.devfs_path()?,
                }]);
            }
        }
        Ok(Vec::new())
    }

    fn is_char_device(name: impl Into<PathBuf>) -> bool {
        use std::os::unix::fs::FileTypeExt;

        match std::fs::metadata(name.into()) {
            Ok(metadata) => metadata.file_type().is_char_device(),
            Err(_) => false,
        }
    }

    // Get the instance number of the tofino asic and use it to construct a /dev/ path.  As a sanity
    // check, verify that it's a char device.
    pub fn device_path(node: &crate::TofinoNode) -> Result<String, Error> {
        let path = match node.instance {
            Some(instance) => format!("/dev/tofino/{instance}"),
            None => bail!("no tofino present"),
        };

        if !is_char_device(&path) {
            bail!("{path} is not a valid device");
        }

        Ok(path)
    }
}

#[cfg(not(target_os = "illumos"))]
mod plat {
    use anyhow::bail;
    use anyhow::Error;
    use anyhow::Result;

    pub fn get_tofino_nodes() -> Result<Vec<crate::TofinoNode>> {
        bail!("tofino asic not supported on this platform")
    }

    pub fn device_path(_node: &crate::TofinoNode) -> Result<String, Error> {
        bail!("tofino asic not supported on this platform")
    }
}

#[cfg(target_os = "illumos")]
pub fn get_tofino_from_devinfo(
    devinfo: &mut illumos_devinfo::DevInfo,
) -> Result<Option<TofinoNode>> {
    let mut all = plat::get_tofino_nodes_from(devinfo)?;
    Ok(all.pop())
}

pub fn get_tofino() -> Result<Option<TofinoNode>> {
    let mut all = plat::get_tofino_nodes()?;
    Ok(all.pop())
}
