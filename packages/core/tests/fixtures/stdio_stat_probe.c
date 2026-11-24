#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

static int nonblock_is(int fd, int enabled) {
    int flags = fcntl(fd, F_GETFL);
    return flags >= 0 && (!!(flags & O_NONBLOCK)) == enabled;
}

int main(int argc, char **argv) {
    int flags_mode = argc > 1 && strcmp(argv[1], "flags") == 0;
    int alias_mode = flags_mode || (argc > 1 && strcmp(argv[1], "alias") == 0);
    int original_nonblock = 0, alias_nonblock = 0, clear_shared = 0, retained_nonblock = 0, clear_retained = 0;
    int read_fd = STDIN_FILENO;
    int same_identity = 0, same_metadata = 0, same_flags = 0, closed_badf = 0;
    int pipe_owner = -1;
    struct stat status;
    if (fstat(STDIN_FILENO, &status) != 0) {
        perror("fstat stdin");
        return 1;
    }
    if (alias_mode) {
        int original_flags = fcntl(STDIN_FILENO, F_GETFL);
        read_fd = dup(STDIN_FILENO);
        if (read_fd < 0) { perror("dup stdin"); return 5; }
        struct stat before_close;
        if (fstat(read_fd, &before_close) != 0) { perror("fstat alias"); return 6; }
        same_identity = before_close.st_ino == status.st_ino && before_close.st_dev == status.st_dev;
        int alias_flags = fcntl(read_fd, F_GETFL);
        same_flags = original_flags >= 0 && alias_flags >= 0 &&
            (original_flags & O_ACCMODE) == (alias_flags & O_ACCMODE);
        if (flags_mode) {
            if (fcntl(STDIN_FILENO, F_SETFL, original_flags | O_NONBLOCK) != 0) { perror("set nonblock stdin"); return 10; }
            original_nonblock = nonblock_is(STDIN_FILENO, 1);
            alias_nonblock = nonblock_is(read_fd, 1);
            if (fcntl(read_fd, F_SETFL, alias_flags & ~O_NONBLOCK) != 0) { perror("clear nonblock alias"); return 11; }
            clear_shared = nonblock_is(STDIN_FILENO, 0) && nonblock_is(read_fd, 0);
            if (fcntl(STDIN_FILENO, F_SETFL, original_flags | O_NONBLOCK) != 0) { perror("set nonblock stdin again"); return 12; }
        }
        if (close(STDIN_FILENO) != 0) { perror("close original stdin"); return 7; }
        struct stat after_close;
        errno = 0;
        closed_badf = fstat(STDIN_FILENO, &after_close) == -1 && errno == EBADF;
        if (fstat(read_fd, &after_close) != 0) { perror("fstat retained alias"); return 8; }
        same_identity = same_identity && after_close.st_ino == status.st_ino && after_close.st_dev == status.st_dev;
        same_metadata = before_close.st_mode == status.st_mode && after_close.st_mode == status.st_mode &&
            before_close.st_size == status.st_size && after_close.st_size == status.st_size &&
            before_close.st_uid == status.st_uid && after_close.st_uid == status.st_uid &&
            before_close.st_gid == status.st_gid && after_close.st_gid == status.st_gid;
        if (flags_mode) {
            retained_nonblock = nonblock_is(read_fd, 1);
            if (fcntl(read_fd, F_SETFL, original_flags & ~O_NONBLOCK) != 0) { perror("clear retained alias"); return 13; }
            clear_retained = nonblock_is(read_fd, 0);
            errno = 0;
            if (fcntl(STDIN_FILENO, F_SETFL, original_flags) != -1 || errno != EBADF) {
                fprintf(stderr, "closed stdin F_SETFL did not report EBADF\n"); return 14;
            }
        }
        if (S_ISFIFO(status.st_mode)) pipe_owner = status.st_uid == getuid() && status.st_gid == getgid();
    }
    struct stat invalid_status;
    errno = 0;
    int invalid = fstat(-1, &invalid_status) == -1 && errno == EBADF;
    if (!invalid) {
        fprintf(stderr, "invalid fd did not report EBADF\n");
        return 2;
    }
    char input[256];
    size_t bytes = 0;
    while (bytes < sizeof(input)) {
        ssize_t count = read(read_fd, input + bytes, sizeof(input) - bytes);
        if (count < 0) {
            perror("read stdin");
            return 3;
        }
        if (count == 0) break;
        bytes += (size_t)count;
    }
    if (bytes == sizeof(input)) {
        fprintf(stderr, "probe input limit exceeded\n");
        return 4;
    }
    printf("{\"fifo\":%d,\"regular\":%d,\"size\":%lld,\"bytes\":%zu,\"invalid\":%d",
        !!S_ISFIFO(status.st_mode), !!S_ISREG(status.st_mode),
        (long long)status.st_size, bytes, invalid);
    if (alias_mode) {
        if (close(read_fd) != 0) { perror("close alias"); return 9; }
        printf(",\"sameIdentity\":%d,\"sameMetadata\":%d,\"sameFlags\":%d,\"closedBadf\":%d,\"pipeOwner\":%d",
            same_identity, same_metadata, same_flags, closed_badf, pipe_owner);
    }
    if (flags_mode) printf(",\"originalNonblock\":%d,\"aliasNonblock\":%d,\"clearShared\":%d,\"retainedNonblock\":%d,\"clearRetained\":%d",
        original_nonblock, alias_nonblock, clear_shared, retained_nonblock, clear_retained);
    printf("}\n");
    return 0;
}
