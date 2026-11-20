#ifndef AGENTOS_XFSTESTS_SYS_SYSMACROS_H
#define AGENTOS_XFSTESTS_SYS_SYSMACROS_H

#include <sys/types.h>

#define major(device) \
    ((unsigned int)((((device) >> 32) & 0xfffff000ULL) | (((device) >> 8) & 0xfffULL)))
#define minor(device) \
    ((unsigned int)((((device) >> 12) & 0xffffff00ULL) | ((device) & 0xffULL)))
#define makedev(major_id, minor_id) \
    ((dev_t)((((dev_t)(major_id) & 0xfffff000ULL) << 32) | \
             (((dev_t)(major_id) & 0xfffULL) << 8) | \
             (((dev_t)(minor_id) & 0xffffff00ULL) << 12) | \
             ((dev_t)(minor_id) & 0xffULL)))

#endif
