/**
 * Bounded file-backed mmap emulation for AgentOS' single-threaded WASM guests.
 *
 * Linear memory cannot provide host page-fault mappings. This implementation
 * snapshots file bytes into malloc-backed memory and writes MAP_SHARED ranges
 * back on msync()/munmap(). MAP_PRIVATE mappings remain isolated.
 */
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#define MAX_MAPPINGS 1024

struct mapping {
    void *address;
    size_t length;
    int prot;
    int flags;
    int fd;
    off_t offset;
};

static struct mapping mappings[MAX_MAPPINGS];
static size_t mapping_count;

static struct mapping *find_mapping(void *address, size_t length) {
    uintptr_t begin = (uintptr_t)address;
    uintptr_t end;
    if (__builtin_add_overflow(begin, length, &end)) return NULL;
    for (size_t i = 0; i < MAX_MAPPINGS; i++) {
        uintptr_t map_begin = (uintptr_t)mappings[i].address;
        uintptr_t map_end;
        if (!map_begin || __builtin_add_overflow(map_begin, mappings[i].length, &map_end)) continue;
        if (begin >= map_begin && end <= map_end) return &mappings[i];
    }
    return NULL;
}

static int write_back(struct mapping *mapping, void *address, size_t length) {
    if (mapping->fd < 0 || (mapping->flags & MAP_SHARED) == 0 ||
        (mapping->prot & PROT_WRITE) == 0) return 0;
    size_t relative = (uintptr_t)address - (uintptr_t)mapping->address;
    const unsigned char *cursor = address;
    size_t remaining = length;
    while (remaining) {
        ssize_t written = pwrite(mapping->fd, cursor, remaining, mapping->offset + (off_t)relative);
        if (written < 0 && errno == EINTR) continue;
        if (written <= 0) {
            if (written == 0) errno = EIO;
            return -1;
        }
        cursor += written;
        remaining -= (size_t)written;
        relative += (size_t)written;
    }
    return 0;
}

void *mmap(void *address, size_t length, int prot, int flags, int fd, off_t offset) {
    if (address || !length || offset < 0 ||
        ((flags & MAP_PRIVATE) == 0 && (flags & MAP_SHARED) == 0) ||
        ((flags & MAP_PRIVATE) != 0 && (flags & MAP_SHARED) != 0) ||
        (flags & MAP_FIXED) != 0 || (prot & PROT_EXEC) != 0 || prot == PROT_NONE) {
        errno = EINVAL;
        return MAP_FAILED;
    }

    struct mapping *slot = NULL;
    for (size_t i = 0; i < MAX_MAPPINGS; i++) {
        if (!mappings[i].address) {
            slot = &mappings[i];
            break;
        }
    }
    if (!slot) {
        errno = ENOMEM;
        return MAP_FAILED;
    }

    unsigned char *buffer = malloc(length);
    if (!buffer) {
        errno = ENOMEM;
        return MAP_FAILED;
    }
    memset(buffer, 0, length);

    int retained_fd = -1;
    if ((flags & MAP_ANONYMOUS) == 0) {
        retained_fd = dup(fd);
        if (retained_fd < 0) {
            free(buffer);
            return MAP_FAILED;
        }
        size_t remaining = length;
        unsigned char *cursor = buffer;
        off_t read_offset = offset;
        while (remaining) {
            ssize_t count = pread(retained_fd, cursor, remaining, read_offset);
            if (count < 0 && errno == EINTR) continue;
            if (count < 0) {
                close(retained_fd);
                free(buffer);
                return MAP_FAILED;
            }
            if (count == 0) break;
            cursor += count;
            remaining -= (size_t)count;
            read_offset += count;
        }
    }

    *slot = (struct mapping){
        .address = buffer,
        .length = length,
        .prot = prot,
        .flags = flags,
        .fd = retained_fd,
        .offset = offset,
    };
    mapping_count++;
    if (mapping_count == (MAX_MAPPINGS * 9) / 10) {
        fprintf(stderr,
                "agentos: mmap table is %zu/%d full; unmap ranges before the %d-entry limit\n",
                mapping_count, MAX_MAPPINGS, MAX_MAPPINGS);
    }
    return buffer;
}

int msync(void *address, size_t length, int flags) {
    if ((flags & ~(MS_ASYNC | MS_INVALIDATE | MS_SYNC)) != 0 ||
        ((flags & MS_ASYNC) != 0 && (flags & MS_SYNC) != 0)) {
        errno = EINVAL;
        return -1;
    }
    struct mapping *mapping = find_mapping(address, length);
    if (!mapping) {
        errno = ENOMEM;
        return -1;
    }
    return write_back(mapping, address, length);
}

int munmap(void *address, size_t length) {
    struct mapping *mapping = find_mapping(address, length);
    if (!mapping || mapping->address != address || mapping->length != length) {
        errno = EINVAL;
        return -1;
    }
    if (write_back(mapping, address, length) != 0) return -1;
    if (mapping->fd >= 0 && close(mapping->fd) != 0) return -1;
    free(mapping->address);
    memset(mapping, 0, sizeof(*mapping));
    mapping_count--;
    return 0;
}

void *mremap(void *old_address, size_t old_length, size_t new_length, int flags, ...) {
    if (!new_length || (flags & ~(MREMAP_MAYMOVE | MREMAP_FIXED)) != 0 ||
        (flags & MREMAP_FIXED) != 0) {
        errno = EINVAL;
        return MAP_FAILED;
    }
    struct mapping *mapping = find_mapping(old_address, old_length);
    if (!mapping || mapping->address != old_address || mapping->length != old_length) {
        errno = EFAULT;
        return MAP_FAILED;
    }

    /* Preserve dirty shared pages before realloc can discard a shrinking tail. */
    if (write_back(mapping, old_address, old_length) != 0) return MAP_FAILED;

    unsigned char *tail = NULL;
    size_t tail_length = new_length > old_length ? new_length - old_length : 0;
    if (tail_length) {
        tail = calloc(1, tail_length);
        if (!tail) {
            errno = ENOMEM;
            return MAP_FAILED;
        }
        if (mapping->fd >= 0) {
            unsigned char *cursor = tail;
            size_t remaining = tail_length;
            off_t read_offset = mapping->offset + (off_t)old_length;
            while (remaining) {
                ssize_t count = pread(mapping->fd, cursor, remaining, read_offset);
                if (count < 0 && errno == EINTR) continue;
                if (count < 0) {
                    free(tail);
                    return MAP_FAILED;
                }
                if (count == 0) break;
                cursor += count;
                remaining -= (size_t)count;
                read_offset += count;
            }
        }
    }

    unsigned char *resized = realloc(old_address, new_length);
    if (!resized) {
        free(tail);
        errno = ENOMEM;
        return MAP_FAILED;
    }
    if (tail_length) {
        memcpy(resized + old_length, tail, tail_length);
        free(tail);
    }
    mapping->address = resized;
    mapping->length = new_length;
    return resized;
}

int mprotect(void *address, size_t length, int prot) {
    struct mapping *mapping = find_mapping(address, length);
    if (!mapping) {
        errno = ENOMEM;
        return -1;
    }
    if (prot != PROT_READ && prot != (PROT_READ | PROT_WRITE)) {
        errno = ENOTSUP;
        return -1;
    }
    mapping->prot = prot;
    return 0;
}
