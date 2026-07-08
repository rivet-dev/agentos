/* pread_pwrite_access.c — test pread, pwrite, and access syscalls */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/stat.h>

static int failures = 0;

#define OK(name) printf(name ": ok\n")
#define FAIL(name, reason) do { \
    printf(name ": FAIL (%s)\n", reason); failures++; \
} while(0)
#define TEST(name, cond, reason) do { \
    if (cond) OK(name); else FAIL(name, reason); \
} while(0)

int main(void) {
    const char *dir = "/tmp/ppa";
    const char *path = "/tmp/ppa/test.dat";
    const char *noent = "/tmp/ppa/nonexistent";

    mkdir(dir, 0755);

    /* Write initial content: "0123456789abcdef" (16 bytes) */
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    TEST("create", fd >= 0, strerror(errno));
    if (fd < 0) {
        printf("total: 1 failures\n");
        return 1;
    }
    const char *init = "0123456789abcdef";
    write(fd, init, 16);
    close(fd);

    /* --- pread tests --- */
    fd = open(path, O_RDONLY);
    TEST("open_rdonly", fd >= 0, strerror(errno));
    if (fd < 0) {
        FAIL("pread_offset5", "skipped");
        FAIL("pread_cursor", "skipped");
        goto after_pread;
    }

    /* Seek to offset 3 first, so we can verify cursor is unchanged after pread */
    lseek(fd, 3, SEEK_SET);

    /* pread at offset 5 should read "56789" */
    {
        char buf[16] = {0};
        ssize_t n = pread(fd, buf, 5, 5);
        TEST("pread_offset5", n == 5 && memcmp(buf, "56789", 5) == 0,
             n < 0 ? strerror(errno) : "content mismatch");
    }

    /* Cursor should still be at 3 after pread */
    {
        off_t pos = lseek(fd, 0, SEEK_CUR);
        char msg[64];
        snprintf(msg, sizeof(msg), "cursor moved to %ld (expected 3)", (long)pos);
        TEST("pread_cursor", pos == 3, msg);
    }
    close(fd);

after_pread:

    /* --- pwrite tests --- */
    fd = open(path, O_RDWR);
    TEST("open_rdwr", fd >= 0, strerror(errno));
    if (fd < 0) {
        FAIL("pwrite_offset10", "skipped");
        FAIL("pwrite_cursor", "skipped");
        FAIL("pwrite_verify", "skipped");
        goto after_pwrite;
    }

    /* Seek to offset 2 so we can verify cursor is unchanged */
    lseek(fd, 2, SEEK_SET);

    /* pwrite "HELLO" at offset 10 */
    {
        ssize_t n = pwrite(fd, "HELLO", 5, 10);
        TEST("pwrite_offset10", n == 5,
             n < 0 ? strerror(errno) : "wrong byte count");
    }

    /* Cursor should still be at 2 */
    {
        off_t pos = lseek(fd, 0, SEEK_CUR);
        char msg[64];
        snprintf(msg, sizeof(msg), "cursor moved to %ld (expected 2)", (long)pos);
        TEST("pwrite_cursor", pos == 2, msg);
    }

    /* Read full file from offset 0 — should be "0123456789HELLOf" */
    {
        char buf[32] = {0};
        lseek(fd, 0, SEEK_SET);
        ssize_t n = read(fd, buf, sizeof(buf));
        TEST("pwrite_verify", n == 16 && memcmp(buf, "0123456789HELLOf", 16) == 0,
             "content mismatch");
    }
    close(fd);

after_pwrite:

    /* --- access tests --- */
    TEST("access_exists", access(path, F_OK) == 0, strerror(errno));

    {
        int r = access(noent, F_OK);
        int e = errno;
        TEST("access_noent", r == -1 && e == ENOENT,
             r == 0 ? "should fail" : strerror(e));
    }

    TEST("access_read", access(path, R_OK) == 0, strerror(errno));
    TEST("access_write", access(path, W_OK) == 0, strerror(errno));

    /* Cleanup */
    unlink(path);
    rmdir(dir);

    printf("total: %d failures\n", failures);
    return failures > 0 ? 1 : 0;
}
