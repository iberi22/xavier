#!/usr/bin/env python3
"""Override AMD CPPC highest_perf limit at boot.
Fixes Ryzen CPUs stuck at base clock due to BIOS CPPC limits.
"""
import os, struct

MSR_CPPC_CAP1 = 0xC0010293

for cpu in range(os.cpu_count() or 12):
    try:
        fd = os.open(f'/dev/cpu/{cpu}/msr', os.O_RDWR)
        os.lseek(fd, MSR_CPPC_CAP1, os.SEEK_SET)
        data = os.read(fd, 8)
        val = struct.unpack('<Q', data)[0]
        new_val = (val & ~0xFF) | 0xFF  # Set highest_perf to 255 (max)
        os.lseek(fd, MSR_CPPC_CAP1, os.SEEK_SET)
        os.write(fd, struct.pack('<Q', new_val))
        os.close(fd)
    except Exception as e:
        print(f'CPU{cpu}: {e}')
