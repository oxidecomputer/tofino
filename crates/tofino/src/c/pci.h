/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Copyright 2023 Oxide Computer Company
 */

#ifndef TFTOOL_SYS_H
#define TFTOOL_SYS_H

#include <sys/types.h>

extern void *pci_map(const char *path, size_t size);
extern const char *pci_err_msg() ;

#endif
