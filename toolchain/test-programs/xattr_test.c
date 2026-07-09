#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/xattr.h>
#include <unistd.h>

static int fail(const char *operation) {
    fprintf(stderr, "%s failed: errno=%d\n", operation, errno);
    return 1;
}

int main(void) {
    const char *path = "/mnt/test/xattr-probe";
    const char *link_path = "/mnt/test/xattr-probe-link";
    const char *name = "user.agentos";
    const char first[] = "first";
    const char second[] = "second";
    char value[32] = {0};
    char list[128] = {0};

    int fd = open(path, O_CREAT | O_RDWR | O_TRUNC, 0600);
    if (fd < 0) return fail("open");
    if (setxattr(path, name, first, sizeof(first) - 1, XATTR_CREATE) != 0)
        return fail("setxattr create");
    if (setxattr(path, name, first, sizeof(first) - 1, XATTR_CREATE) == 0 || errno != EEXIST)
        return fail("setxattr duplicate create errno");

    ssize_t required = getxattr(path, name, NULL, 0);
    if (required != (ssize_t)(sizeof(first) - 1)) return fail("getxattr size");
    if (getxattr(path, name, value, 2) >= 0 || errno != ERANGE)
        return fail("getxattr range errno");
    if (getxattr(path, name, value, sizeof(value)) != required ||
        memcmp(value, first, sizeof(first) - 1) != 0)
        return fail("getxattr value");

    ssize_t list_size = listxattr(path, NULL, 0);
    if (list_size != (ssize_t)(strlen(name) + 1)) return fail("listxattr size");
    if (listxattr(path, list, sizeof(list)) != list_size || strcmp(list, name) != 0)
        return fail("listxattr value");

    if (link(path, link_path) != 0) return fail("link");
    memset(value, 0, sizeof(value));
    if (getxattr(link_path, name, value, sizeof(value)) != required ||
        memcmp(value, first, sizeof(first) - 1) != 0)
        return fail("hardlink getxattr");

    if (fsetxattr(fd, name, second, sizeof(second) - 1, XATTR_REPLACE) != 0)
        return fail("fsetxattr replace");
    memset(value, 0, sizeof(value));
    if (fgetxattr(fd, name, value, sizeof(value)) != (ssize_t)(sizeof(second) - 1) ||
        memcmp(value, second, sizeof(second) - 1) != 0)
        return fail("fgetxattr value");
    memset(list, 0, sizeof(list));
    if (flistxattr(fd, list, sizeof(list)) != list_size || strcmp(list, name) != 0)
        return fail("flistxattr value");

    if (fremovexattr(fd, name) != 0) return fail("fremovexattr");
    if (getxattr(path, name, value, sizeof(value)) >= 0 || errno != ENODATA)
        return fail("getxattr missing errno");
    close(fd);
    unlink(link_path);
    unlink(path);
    puts("xattr: ok");
    return 0;
}
