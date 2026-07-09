#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

uint32_t __agentos_host_path_renameat2(
    uint32_t olddirfd, const char *oldpath, size_t oldpath_len,
    uint32_t newdirfd, const char *newpath, size_t newpath_len,
    uint32_t flags) __attribute__((
        __import_module__("host_fs"),
        __import_name__("path_renameat2")));

int renameat2(int olddirfd, const char *oldpath, int newdirfd,
              const char *newpath, unsigned int flags) {
    uint32_t error = __agentos_host_path_renameat2(
        (uint32_t)olddirfd, oldpath, strlen(oldpath),
        (uint32_t)newdirfd, newpath, strlen(newpath), flags);
    if (error != 0) {
        errno = (int)error;
        return -1;
    }
    return 0;
}
