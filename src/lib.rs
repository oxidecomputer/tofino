// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2023 Oxide Computer Company

use anyhow::{Error, Result};

#[cfg(target_os = "illumos")]
mod plat {
    use std::path::PathBuf;

    use anyhow::{anyhow, bail, Context, Error, Result};
    use illumos_devinfo::DevInfo;

    #[derive(Clone, Debug, PartialEq)]
    pub struct TofinoNode {
        pub name: String,
        pub driver: Option<String>,
        pub instance: Option<i32>,
        pub devfs_path: String,
    }

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
    fn get_tofino_nodes() -> Result<Vec<TofinoNode>> {
        let mut device_info =
            DevInfo::new_force_load().with_context(|| "loading devinfo map")?;

        let mut node_walker = device_info.walk_node();
        while let Some(node) = node_walker
            .next()
            .transpose()
            .map_err(|e| anyhow!("unable to walk device tree: {:?}", e))?
        {
            if is_tofino_node_name(&node.node_name()) {
                return Ok(vec![TofinoNode {
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
    pub fn device_path() -> Result<String, Error> {
        let tf = match get_tofino_nodes() {
            Err(e) => bail!("getting tofino device node: {e:?}"),
            Ok(mut nodes) => {
                nodes.pop().ok_or_else(|| anyhow!("no tofino found"))?
            }
        };

        let path = match tf.instance {
            Some(instance) => format!("/dev/tofino/{instance}"),
            None => bail!("no tofino present"),
        };

        if !is_char_device(&path) {
            bail!("{path} is not a valid device");
        }

        Ok(path)
    }

    pub fn has_tofino() -> bool {
        match get_tofino_nodes() {
            Ok(nodes) if !nodes.is_empty() => {
                nodes[0].driver.is_some() && nodes[0].instance.is_some()
            }
            _ => false,
        }
    }
}

#[cfg(not(target_os = "illumos"))]
mod plat {
    use anyhow::bail;
    use anyhow::Error;
    use anyhow::Result;

    pub fn device_path() -> Result<String, Error> {
        bail!("tofino asic not supported on this platform")
    }

    pub fn has_tofino() -> bool {
        false
    }
}

pub fn device_path() -> Result<String, Error> {
    plat::device_path()
}

pub fn has_tofino() -> bool {
    plat::has_tofino()
}
