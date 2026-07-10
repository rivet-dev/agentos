/*
 * POSIX ttyname(3) for AgentOS virtual terminals. isatty(3) is authoritative;
 * guest TTY descriptors project through the standard /dev/tty path.
 */

#include <errno.h>
#include <limits.h>
#include <stddef.h>
#include <string.h>
#include <unistd.h>

int ttyname_r(int fd, char *name, size_t size) {
    static const char path[] = "/dev/tty";
    if (!isatty(fd))
        return errno != 0 ? errno : ENOTTY;
    if (name == NULL)
        return EINVAL;
    if (size < sizeof(path))
        return ERANGE;
    memcpy(name, path, sizeof(path));
    return 0;
}

char *ttyname(int fd) {
    static char path[TTY_NAME_MAX];
    int error = ttyname_r(fd, path, sizeof(path));
    if (error != 0) {
        errno = error;
        return NULL;
    }
    return path;
}
