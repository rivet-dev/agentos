#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int check_chown32(const char *path) {
    struct stat st;

    if (chown(path, 98789, 98789) != 0) {
        perror("feature: chown");
        return 1;
    }
    if (lstat(path, &st) != 0) {
        perror("feature: lstat");
        return 1;
    }
    return st.st_uid == 98789 && st.st_gid == 98789 ? 0 : 1;
}

static int check_truncate64(const char *path) {
    const off_t length = (off_t)4294967307LL;
    struct stat st;
    int fd = open(path, O_CREAT | O_RDWR, 0600);

    if (fd < 0) {
        perror("feature: open");
        return 1;
    }
    if (ftruncate(fd, length) != 0 || fstat(fd, &st) != 0) {
        perror("feature: ftruncate64");
        close(fd);
        return 1;
    }
    close(fd);
    return st.st_size == length ? 0 : 1;
}

static void usage(const char *program) {
    fprintf(stderr, "Usage: %s [-v] -<A|c|g|G|o|p|P|q|r|R|s|t|u|U|w> [file]\n", program);
}

int main(int argc, char **argv) {
    const char *flag = NULL;
    const char *path = NULL;

    for (int index = 1; index < argc; index++) {
        if (!strcmp(argv[index], "-v"))
            continue;
        if (argv[index][0] == '-' && argv[index][1] && !argv[index][2]) {
            if (flag) {
                usage(argv[0]);
                return 1;
            }
            flag = argv[index];
        } else if (!path) {
            path = argv[index];
        } else {
            usage(argv[0]);
            return 1;
        }
    }

    if (!flag) {
        usage(argv[0]);
        return 1;
    }
    switch (flag[1]) {
    case 'c':
        return path ? check_chown32(path) : 1;
    case 't':
        return path ? check_truncate64(path) : 1;
    case 's':
        if (path) return 1;
        printf("%d\n", getpagesize());
        return 0;
    case 'w':
        if (path) return 1;
        printf("%zu\n", sizeof(long) * CHAR_BIT);
        return 0;
    case 'o': {
        long count;
        if (path) return 1;
#ifdef _SC_NPROCESSORS_ONLN
        count = sysconf(_SC_NPROCESSORS_ONLN);
#else
        count = 1;
#endif
        printf("%ld\n", count > 0 ? count : 1);
        return 0;
    }
    case 'A':
    case 'g':
    case 'G':
    case 'p':
    case 'P':
    case 'q':
    case 'r':
    case 'R':
    case 'u':
    case 'U':
        return 1;
    default:
        usage(argv[0]);
        return 1;
    }
}
