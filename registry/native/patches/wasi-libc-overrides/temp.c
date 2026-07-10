#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* POSIX mkstemp(3p)/mkdtemp(3p), bounded to 128 collision attempts. */
static int randomize(char *value, int suffix_length) {
    static const char alphabet[] = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    size_t length = strlen(value);
    if (suffix_length < 0 || length < (size_t)suffix_length + 6 ||
        memcmp(value + length - suffix_length - 6, "XXXXXX", 6) != 0) {
        errno = EINVAL;
        return -1;
    }
    char *slot = value + length - suffix_length - 6;
    for (int index = 0; index < 6; index++)
        slot[index] = alphabet[arc4random_uniform(sizeof(alphabet) - 1)];
    return 0;
}

int mkostemps(char *value, int suffix_length, int flags) {
    char *slot;
    size_t length;
    if (value == NULL) {
        errno = EINVAL;
        return -1;
    }
    length = strlen(value);
    if (length < (size_t)suffix_length + 6) {
        errno = EINVAL;
        return -1;
    }
    slot = value + length - suffix_length - 6;
    for (int attempt = 0; attempt < 128; attempt++) {
        memcpy(slot, "XXXXXX", 6);
        if (randomize(value, suffix_length) < 0)
            return -1;
        int fd = open(value, O_RDWR | O_CREAT | O_EXCL | flags, 0600);
        if (fd >= 0 || errno != EEXIST)
            return fd;
    }
    memcpy(slot, "XXXXXX", 6);
    errno = EEXIST;
    return -1;
}

int mkstemps(char *value, int suffix_length) { return mkostemps(value, suffix_length, 0); }
int mkostemp(char *value, int flags) { return mkostemps(value, 0, flags); }
int mkstemp(char *value) { return mkostemps(value, 0, 0); }

char *mkdtemp(char *value) {
    size_t length;
    char *slot;
    if (value == NULL || (length = strlen(value)) < 6) {
        errno = EINVAL;
        return NULL;
    }
    slot = value + length - 6;
    for (int attempt = 0; attempt < 128; attempt++) {
        memcpy(slot, "XXXXXX", 6);
        if (randomize(value, 0) < 0)
            return NULL;
        if (mkdir(value, 0700) == 0)
            return value;
        if (errno != EEXIST)
            return NULL;
    }
    memcpy(slot, "XXXXXX", 6);
    errno = EEXIST;
    return NULL;
}

char *mktemp(char *value) {
    int fd = mkstemp(value);
    if (fd < 0)
        return value;
    close(fd);
    unlink(value);
    return value;
}
