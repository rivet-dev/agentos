#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int h_errno;

#ifdef __wasm__
__attribute__((import_module("host_fs"), import_name("path_mknod")))
uint32_t agentos_path_mknod(uint32_t fd, const char *path, uint32_t path_len,
                            uint32_t mode, uint64_t device);
#endif

#define AT_FDCWD_SENTINEL UINT32_MAX

struct hostent *agentos_gethostbyname(const char *name) {
    static struct hostent host;
    static struct in_addr address;
    static char *addresses[] = { (char *)&address, NULL };
    static char *aliases[] = { NULL };
    struct addrinfo hints = {0};
    struct addrinfo *result = NULL;
    int error;

    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    error = getaddrinfo(name, NULL, &hints, &result);
    if (error != 0 || result == NULL) {
        h_errno = error;
        return NULL;
    }
    address = ((struct sockaddr_in *)result->ai_addr)->sin_addr;
    freeaddrinfo(result);
    host.h_name = (char *)name;
    host.h_aliases = aliases;
    host.h_addrtype = AF_INET;
    host.h_length = sizeof(address);
    host.h_addr_list = addresses;
    return &host;
}

char *agentos_strsignal(int signal_number) {
    static char description[32];

    switch (signal_number) {
    case SIGIO:
        return "I/O possible";
    case SIGTERM:
        return "Terminated";
    case SIGKILL:
        return "Killed";
    default:
        snprintf(description, sizeof(description), "signal %d", signal_number);
        return description;
    }
}

static int agentos_host_mknod(uint32_t dirfd, const char *path, mode_t mode,
                             dev_t device) {
#ifdef __wasm__
    uint32_t error = agentos_path_mknod(dirfd, path, (uint32_t)strlen(path),
                                        (uint32_t)mode, (uint64_t)device);
    if (error != 0) {
        errno = (int)error;
        return -1;
    }
    return 0;
#else
    (void)dirfd;
    (void)path;
    (void)mode;
    (void)device;
    errno = ENOSYS;
    return -1;
#endif
}

int agentos_mknod(const char *path, mode_t mode, dev_t device) {
    int cwd_fd = open(".", O_RDONLY | O_DIRECTORY);
    int result;
    int saved_errno;

    if (cwd_fd < 0)
        return -1;
    result = agentos_host_mknod((uint32_t)cwd_fd, path, mode, device);
    saved_errno = errno;
    close(cwd_fd);
    errno = saved_errno;
    return result;
}

int agentos_mknodat(int dirfd, const char *path, mode_t mode, dev_t device) {
    int fd;

    if ((mode & S_IFMT) != S_IFREG || device != 0) {
        if (dirfd == AT_FDCWD)
            return agentos_mknod(path, mode, device);
        return agentos_host_mknod((uint32_t)dirfd, path, mode, device);
    }
    fd = openat(dirfd, path, O_CREAT | O_EXCL | O_WRONLY, mode & 07777);
    if (fd < 0)
        return -1;
    return close(fd);
}

static void print_message(const char *format, va_list args, int include_errno) {
    if (format) {
        vfprintf(stderr, format, args);
        if (include_errno) fputs(": ", stderr);
    }
    if (include_errno) fputs(strerror(errno), stderr);
    fputc('\n', stderr);
}

void vwarn(const char *format, va_list args) { print_message(format, args, 1); }
void vwarnx(const char *format, va_list args) { print_message(format, args, 0); }

void warn(const char *format, ...) {
    va_list args;
    va_start(args, format);
    vwarn(format, args);
    va_end(args);
}

void warnx(const char *format, ...) {
    va_list args;
    va_start(args, format);
    vwarnx(format, args);
    va_end(args);
}

void verr(int status, const char *format, va_list args) {
    vwarn(format, args);
    exit(status);
}

void verrx(int status, const char *format, va_list args) {
    vwarnx(format, args);
    exit(status);
}

void err(int status, const char *format, ...) {
    va_list args;
    va_start(args, format);
    verr(status, format, args);
}

void errx(int status, const char *format, ...) {
    va_list args;
    va_start(args, format);
    verrx(status, format, args);
}
