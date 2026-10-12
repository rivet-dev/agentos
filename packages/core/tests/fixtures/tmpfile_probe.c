#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define CHECK(condition) do { \
    if (!(condition)) { \
        fprintf(stderr, "line %d: %s (errno=%d: %s)\n", \
                __LINE__, #condition, errno, strerror(errno)); \
        return 1; \
    } \
} while (0)

static int visible_entries(const char *path, const char *expected) {
    DIR *dir = opendir(path);
    CHECK(dir != NULL);
    int count = 0;
    struct dirent *entry;
    errno = 0;
    while ((entry = readdir(dir)) != NULL) {
        if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, "..")) continue;
        CHECK(expected != NULL && !strcmp(entry->d_name, expected));
        count++;
    }
    CHECK(errno == 0);
    CHECK(closedir(dir) == 0);
    CHECK(count == (expected != NULL ? 1 : 0));
    return 0;
}

int main(int argc, char **argv) {
    CHECK(argc == 3);
    const char *base = argv[1];
    const char *scenario = argv[2];
    char path[512];
    CHECK(snprintf(path, sizeof(path), "%s/.agentos-tmpfile-user-owned", base) < sizeof(path));

    if (!strcmp(scenario, "unsupported")) {
        CHECK(snprintf(path, sizeof(path), "%s/ordinary", base) < sizeof(path));
        int ordinary = open(path, O_CREAT | O_EXCL | O_RDWR, 0600);
        CHECK(ordinary >= 0);
        CHECK(write(ordinary, "ok", 2) == 2);
        CHECK(close(ordinary) == 0);
        CHECK(visible_entries(base, "ordinary") == 0);
        CHECK(unlink(path) == 0);
        errno = 0;
        CHECK(open(base, O_TMPFILE | O_RDWR, 0600) == -1);
        CHECK(errno == EOPNOTSUPP);
        puts("EOPNOTSUPP; ordinary file remains supported");
        return 0;
    }

    CHECK(mkdir(base, 0700) == 0);
    int fd = open(base, O_TMPFILE | O_RDWR, 0600);
    CHECK(fd >= 0);
    CHECK(write(fd, "first", 5) == 5);

    if (!strcmp(scenario, "namespace")) {
        int ordinary = open(path, O_CREAT | O_EXCL | O_RDWR, 0600);
        CHECK(ordinary >= 0);
        CHECK(write(ordinary, "ordinary", 8) == 8);
        CHECK(close(ordinary) == 0);
        CHECK(visible_entries(base, ".agentos-tmpfile-user-owned") == 0);
        CHECK(unlink(path) == 0);
        CHECK(visible_entries(base, NULL) == 0);
        // An unnamed inode must not prevent removal of its original directory.
        CHECK(rmdir(base) == 0);
        char data[5];
        CHECK(pread(fd, data, sizeof(data), 0) == sizeof(data));
        CHECK(!memcmp(data, "first", sizeof(data)));
        CHECK(close(fd) == 0);
        puts("pathless namespace; prefix file visible; removed parent lifetime");
        return 0;
    }

    CHECK(!strcmp(scenario, "lifetime"));
    struct stat before, linked, after;
    CHECK(fstat(fd, &before) == 0);
    CHECK(before.st_nlink == 0);
    CHECK(visible_entries(base, NULL) == 0);
    CHECK(snprintf(path, sizeof(path), "%s/published", base) < sizeof(path));
    if (linkat(fd, "", AT_FDCWD, path, AT_EMPTY_PATH) != 0) {
        // Linux permits the /proc form without CAP_DAC_READ_SEARCH.
        char descriptor_path[64];
        CHECK(snprintf(descriptor_path, sizeof(descriptor_path), "/proc/self/fd/%d", fd) < sizeof(descriptor_path));
        CHECK(linkat(AT_FDCWD, descriptor_path, AT_FDCWD, path, AT_SYMLINK_FOLLOW) == 0);
    }
    CHECK(stat(path, &linked) == 0);
    CHECK(before.st_dev == linked.st_dev && before.st_ino == linked.st_ino);
    CHECK(linked.st_nlink == 1 && linked.st_size == 5);
    CHECK(fstat(fd, &after) == 0);
    CHECK(after.st_ino == before.st_ino && after.st_nlink == 1);
    CHECK(visible_entries(base, "published") == 0);

    int alias = open(path, O_RDWR);
    CHECK(alias >= 0);
    char data[5];
    CHECK(pread(alias, data, sizeof(data), 0) == sizeof(data));
    CHECK(!memcmp(data, "first", sizeof(data)));
    CHECK(pwrite(alias, "other", 5, 0) == 5);
    CHECK(pread(fd, data, sizeof(data), 0) == sizeof(data));
    CHECK(!memcmp(data, "other", sizeof(data)));
    CHECK(unlink(path) == 0);
    errno = 0;
    CHECK(stat(path, &after) == -1 && errno == ENOENT);
    CHECK(fstat(fd, &after) == 0);
    CHECK(after.st_ino == before.st_ino && after.st_nlink == 0);
    CHECK(close(fd) == 0);
    CHECK(pread(alias, data, sizeof(data), 0) == sizeof(data));
    CHECK(!memcmp(data, "other", sizeof(data)));
    CHECK(close(alias) == 0);
    CHECK(visible_entries(base, NULL) == 0);
    CHECK(rmdir(base) == 0);
    puts("same inode and content through link; unlink survives until final close");
    return 0;
}
