/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Copyright 2025 Oxide Computer Company
 */

#include <stdio.h>
#include <stdbool.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <string.h>
#include <strings.h>
#include <sys/mman.h>
#include "pci.h"

#define MAX_ERR_LEN 256
static char err_msg[MAX_ERR_LEN + 1];

const char *
pci_err_msg() {
	if (err_msg[0] == 0) {
		return NULL;
	} else {
		return err_msg;
	}
}

void *
pci_map(int fd, size_t len)
{
	bzero(err_msg, MAX_ERR_LEN + 1);
	caddr_t base = mmap(NULL, len, PROT_READ | PROT_WRITE,
	    MAP_SHARED, fd, 0);
	if (base == MAP_FAILED) {
		snprintf(err_msg, MAX_ERR_LEN,
		    "failed to map device: %s", strerror(errno));
		return NULL;
	}

	return base;
}

void
pci_unmap(void *base, size_t len)
{
	munmap(base, len);
}
