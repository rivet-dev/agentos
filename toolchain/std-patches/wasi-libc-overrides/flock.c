#include <errno.h>
#include <stdint.h>
#include <sys/file.h>

__attribute__((import_module("host_process"), import_name("fd_flock")))
uint32_t __agentos_host_fd_flock(uint32_t fd, uint32_t operation);

int flock(int fd, int operation) {
    uint32_t error = __agentos_host_fd_flock((uint32_t)fd, (uint32_t)operation);
    if (error != 0) {
        errno = (int)error;
        return -1;
    }
    return 0;
}
