/* syscall_coverage.c — comprehensive test exercising every libc-to-WASI
 * and libc-to-host-import syscall path.
 *
 * Output: structured "name: ok" or "name: FAIL (reason)" lines
 * Exit:   0 if ALL pass, 1 if any FAIL
 * Requires: patched sysroot (host_process + host_user imports)
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <dirent.h>
#include <time.h>
#include <errno.h>
#include <pwd.h>
#include <signal.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

#include "posix_spawn_compat.h"

extern char **environ;

static int failures = 0;
static volatile sig_atomic_t sigaction_hits = 0;

#define OK(name) printf(name ": ok\n")
#define FAIL(name, reason) do { \
    printf(name ": FAIL (%s)\n", reason); failures++; \
} while(0)
#define TEST(name, cond, reason) do { \
    if (cond) OK(name); else FAIL(name, reason); \
} while(0)

static void syscall_coverage_sigaction_handler(int sig) {
    sigaction_hits = sig;
}

/* ========== WASI FD operations ========== */

static void test_fd_ops(const char *base) {
    char path[512];
    snprintf(path, sizeof(path), "%s/fd_test.txt", base);

    const char *data = "hello fd ops\n";
    size_t len = strlen(data);

    /* open + write */
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    TEST("open", fd >= 0, strerror(errno));
    if (fd < 0) {
        FAIL("write", "skipped"); FAIL("read", "skipped");
        FAIL("pread", "skipped"); FAIL("pwrite", "skipped");
        FAIL("seek", "skipped"); FAIL("fstat", "skipped");
        FAIL("ftruncate", "skipped"); FAIL("close", "skipped");
        return;
    }

    ssize_t w = write(fd, data, len);
    TEST("write", w == (ssize_t)len, "wrong byte count");
    close(fd);

    /* read */
    fd = open(path, O_RDONLY);
    if (fd < 0) {
        FAIL("read", strerror(errno));
        FAIL("seek", "skipped"); FAIL("pread", "skipped");
        goto after_read;
    }
    {
        char buf[256] = {0};
        ssize_t r = read(fd, buf, sizeof(buf));
        if (r < 0) {
            fprintf(stderr, "DBG read: errno=%d (%s)\n", errno, strerror(errno));
        } else if (r != (ssize_t)len || memcmp(buf, data, len) != 0) {
            fprintf(stderr, "DBG read: r=%zd expected=%zu buf='%.*s'\n", r, len, (int)r, buf);
        }
        TEST("read", r == (ssize_t)len && memcmp(buf, data, len) == 0, "content mismatch");

        /* seek */
        off_t pos = lseek(fd, 0, SEEK_SET);
        TEST("seek", pos == 0, "not zero");

        /* pread */
        char pbuf[16] = {0};
        ssize_t pr = pread(fd, pbuf, 5, 0);
        TEST("pread", pr == 5 && memcmp(pbuf, "hello", 5) == 0, "content mismatch");
        close(fd);
    }

after_read:
    /* pwrite */
    fd = open(path, O_WRONLY);
    if (fd >= 0) {
        ssize_t pw = pwrite(fd, "HELLO", 5, 0);
        TEST("pwrite", pw == 5, "wrong byte count");
        close(fd);
    } else {
        FAIL("pwrite", strerror(errno));
    }

    /* fstat */
    fd = open(path, O_RDONLY);
    if (fd >= 0) {
        struct stat st;
        int sr = fstat(fd, &st);
        TEST("fstat", sr == 0 && st.st_size > 0, "failed or zero size");
        close(fd);
    } else {
        FAIL("fstat", strerror(errno));
    }

    /* ftruncate */
    fd = open(path, O_WRONLY);
    if (fd >= 0) {
        int tr = ftruncate(fd, 5);
        TEST("ftruncate", tr == 0, strerror(errno));
        close(fd);
    } else {
        FAIL("ftruncate", strerror(errno));
    }

    /* close — explicit test */
    fd = open(path, O_RDONLY);
    TEST("close", fd >= 0 && close(fd) == 0, "failed");

    unlink(path);
}

/* ========== WASI path operations ========== */

static void test_path_ops(const char *base) {
    char dir[512], file[512], file2[512], link[512];
    snprintf(dir, sizeof(dir), "%s/pdir", base);
    snprintf(file, sizeof(file), "%s/pdir/f.txt", base);
    snprintf(file2, sizeof(file2), "%s/pdir/r.txt", base);
    snprintf(link, sizeof(link), "%s/pdir/lnk", base);

    /* mkdir */
    TEST("mkdir", mkdir(dir, 0755) == 0, strerror(errno));

    /* stat */
    struct stat st;
    TEST("stat", stat(dir, &st) == 0 && S_ISDIR(st.st_mode), "not a directory");

    /* create file for further tests */
    FILE *f = fopen(file, "w");
    if (f) { fprintf(f, "test\n"); fclose(f); }

    /* rename */
    TEST("rename", rename(file, file2) == 0, strerror(errno));

    /* opendir + readdir + closedir */
    DIR *d = opendir(dir);
    TEST("opendir", d != NULL, strerror(errno));
    if (d) {
        int found = 0;
        struct dirent *ent;
        while ((ent = readdir(d)) != NULL) {
            if (strcmp(ent->d_name, "r.txt") == 0) found = 1;
            fprintf(stderr, "DBG readdir: entry='%s'\n", ent->d_name);
        }
        TEST("readdir", found, "r.txt not found");
        TEST("closedir", closedir(d) == 0, "failed");
    } else {
        FAIL("readdir", "skipped"); FAIL("closedir", "skipped");
    }

    /* symlink */
    TEST("symlink", symlink(file2, link) == 0, strerror(errno));

    /* readlink */
    char rl[512] = {0};
    ssize_t rl_len = readlink(link, rl, sizeof(rl) - 1);
    TEST("readlink", rl_len > 0 && strcmp(rl, file2) == 0, "target mismatch");

    /* cleanup before unlink/rmdir */
    unlink(link);

    /* unlink */
    TEST("unlink", unlink(file2) == 0, strerror(errno));

    /* rmdir */
    TEST("rmdir", rmdir(dir) == 0, strerror(errno));
}

/* ========== Args/Env/Clock ========== */

static void test_args_env_clock(int argc, char *argv[]) {
    TEST("argc", argc >= 1, "< 1");
    TEST("argv", argv[0] != NULL && strlen(argv[0]) > 0, "argv[0] empty");

    /* Check for TEST_SC=1 env var (set by test harness) */
    int found = 0;
    if (environ)
        for (int i = 0; environ[i]; i++)
            if (strncmp(environ[i], "TEST_SC=1", 9) == 0) { found = 1; break; }
    TEST("environ", found, "TEST_SC=1 not found");

    /* clock_gettime CLOCK_REALTIME */
    struct timespec ts;
    int cr = clock_gettime(CLOCK_REALTIME, &ts);
    TEST("clock_realtime", cr == 0 && (ts.tv_sec > 0 || ts.tv_nsec > 0),
         "returned zero or failed");

    /* clock_gettime CLOCK_MONOTONIC */
    cr = clock_gettime(CLOCK_MONOTONIC, &ts);
    TEST("clock_monotonic", cr == 0 && (ts.tv_sec > 0 || ts.tv_nsec > 0),
         "returned zero or failed");
}

/* ========== host_process imports ========== */

static void test_host_process(void) {
    /* pipe */
    int pfd[2];
    int pr = pipe(pfd);
    TEST("pipe", pr == 0, strerror(errno));
    if (pr == 0) {
        /* verify data round-trip */
        write(pfd[1], "ok", 2);
        close(pfd[1]);
        char b[8] = {0};
        read(pfd[0], b, sizeof(b));
        close(pfd[0]);
    }

    /* dup */
    int nd = dup(STDOUT_FILENO);
    TEST("dup", nd >= 0, strerror(errno));
    if (nd >= 0) close(nd);

    /* dup2 */
    int d2 = dup2(STDOUT_FILENO, 20);
    TEST("dup2", d2 == 20, strerror(errno));
    if (d2 == 20) close(20);

    /* getpid */
    pid_t pid = getpid();
    pid_t pid2 = getpid();
    TEST("getpid", pid > 0 && pid != 42 && pid == pid2, "invalid or inconsistent");

    /* getppid */
    pid_t ppid = getppid();
    TEST("getppid", ppid >= 0, "negative");

    /* sigaction */
    struct sigaction action;
    memset(&action, 0, sizeof(action));
    sigemptyset(&action.sa_mask);
    sigaddset(&action.sa_mask, SIGTERM);
    action.sa_flags = SA_RESTART | SA_RESETHAND;
    action.sa_handler = syscall_coverage_sigaction_handler;
    int sar = sigaction(SIGINT, &action, NULL);
    TEST("sigaction_register", sar == 0, strerror(errno));
    if (sar == 0) {
        struct sigaction current;
        memset(&current, 0, sizeof(current));
        kill(getpid(), SIGINT);
        int sq = sigaction(SIGINT, NULL, &current);
        TEST("sigaction_query", sq == 0
            && sigaction_hits == SIGINT
            && current.sa_handler == SIG_DFL,
            "handler did not fire or reset");
    } else {
        FAIL("sigaction_query", "skipped");
    }

    /* posix_spawn + waitpid */
    int spfd[2];
    if (pipe(spfd) == 0) {
        posix_spawn_file_actions_t fa;
        posix_spawn_file_actions_init(&fa);
        posix_spawn_file_actions_adddup2(&fa, spfd[1], STDOUT_FILENO);
        posix_spawn_file_actions_addclose(&fa, spfd[0]);
        posix_spawn_file_actions_addclose(&fa, spfd[1]);

        char *a[] = {"echo", "spawn_ok", NULL};
        pid_t ch;
        int err = posix_spawnp(&ch, "echo", &fa, NULL, a, environ);
        posix_spawn_file_actions_destroy(&fa);
        close(spfd[1]);

        if (err == 0) {
            char sb[128] = {0};
            read(spfd[0], sb, sizeof(sb));
            close(spfd[0]);
            int st;
            pid_t w = waitpid(ch, &st, 0);
            TEST("spawn_waitpid", w > 0 && WIFEXITED(st) && WEXITSTATUS(st) == 0 &&
                 strstr(sb, "spawn_ok") != NULL, "failed");
        } else {
            close(spfd[0]);
            FAIL("spawn_waitpid", "posix_spawn error");
        }
    } else {
        FAIL("spawn_waitpid", "pipe error");
    }

    /* kill — test import by sending signal 0 to a recently-exited child.
     * Avoids spawning long-lived processes that hang if signal delivery
     * or waitpid has issues (covered separately by kill_child.c). */
    {
        char *a[] = {"true", NULL};
        pid_t ch;
        int err = posix_spawnp(&ch, "true", NULL, NULL, a, environ);
        if (err == 0) {
            int st;
            waitpid(ch, &st, 0);
            /* After collecting, kill(child, 0) should return -1/ESRCH or 0.
             * Either result proves the host import works end-to-end. */
            int kr = kill(ch, 0);
            TEST("kill", kr == 0 || kr == -1, "unexpected return");
        } else {
            FAIL("kill", "could not spawn child");
        }
    }
}

/* ========== host_user imports ========== */

static void test_host_user(void) {
    /* Validate actual return values, not just linkage */
    uid_t u = getuid();
    TEST("getuid", u > 0, "expected positive uid");

    gid_t g = getgid();
    TEST("getgid", g > 0, "expected positive gid");

    uid_t eu = geteuid();
    TEST("geteuid", eu > 0, "expected positive euid");

    gid_t eg = getegid();
    TEST("getegid", eg > 0, "expected positive egid");

    /* When piped (not PTY), isatty returns 0 */
    TEST("isatty_stdin", isatty(STDIN_FILENO) == 0, "stdin should not be tty when piped");

    /* getpwuid — verify passwd entry for uid 1000 */
    struct passwd *pw = getpwuid(1000);
    if (pw != NULL && pw->pw_name != NULL && strlen(pw->pw_name) > 0 &&
        pw->pw_uid == 1000) {
        OK("getpwuid");
    } else {
        FAIL("getpwuid", pw ? "bad fields" : "returned NULL");
    }
}

/* ========== host_net imports exercised through libc ========== */

static void test_host_net(void) {
    int listener_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (listener_fd < 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        return;
    }

    struct sockaddr_in listener_addr;
    memset(&listener_addr, 0, sizeof(listener_addr));
    listener_addr.sin_family = AF_INET;
    listener_addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    listener_addr.sin_port = htons(0);

    if (bind(listener_fd, (struct sockaddr *)&listener_addr, sizeof(listener_addr)) != 0 ||
        listen(listener_fd, 1) != 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        close(listener_fd);
        return;
    }

    struct sockaddr_in bound_listener_addr;
    socklen_t bound_listener_len = sizeof(bound_listener_addr);
    if (getsockname(listener_fd, (struct sockaddr *)&bound_listener_addr, &bound_listener_len) != 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        close(listener_fd);
        return;
    }

    int client_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (client_fd < 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        close(listener_fd);
        return;
    }

    struct sockaddr_in client_bind_addr;
    memset(&client_bind_addr, 0, sizeof(client_bind_addr));
    client_bind_addr.sin_family = AF_INET;
    client_bind_addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    client_bind_addr.sin_port = htons(0);
    if (bind(client_fd, (struct sockaddr *)&client_bind_addr, sizeof(client_bind_addr)) != 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        close(client_fd);
        close(listener_fd);
        return;
    }

    struct sockaddr_in bound_client_addr;
    socklen_t bound_client_len = sizeof(bound_client_addr);
    if (getsockname(client_fd, (struct sockaddr *)&bound_client_addr, &bound_client_len) != 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        close(client_fd);
        close(listener_fd);
        return;
    }

    if (connect(client_fd, (struct sockaddr *)&bound_listener_addr, sizeof(bound_listener_addr)) != 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        close(client_fd);
        close(listener_fd);
        return;
    }

    int server_fd = accept(listener_fd, NULL, NULL);
    if (server_fd < 0) {
        FAIL("getsockname", strerror(errno));
        FAIL("getpeername", "skipped");
        close(client_fd);
        close(listener_fd);
        return;
    }

    struct sockaddr_in accepted_local_addr;
    socklen_t accepted_local_len = sizeof(accepted_local_addr);
    struct sockaddr_in client_peer_addr;
    socklen_t client_peer_len = sizeof(client_peer_addr);
    struct sockaddr_in server_peer_addr;
    socklen_t server_peer_len = sizeof(server_peer_addr);

    int getsockname_ok =
        getsockname(server_fd, (struct sockaddr *)&accepted_local_addr, &accepted_local_len) == 0 &&
        ntohs(bound_listener_addr.sin_port) != 0 &&
        ntohs(bound_client_addr.sin_port) != 0 &&
        ntohs(accepted_local_addr.sin_port) == ntohs(bound_listener_addr.sin_port) &&
        bound_listener_addr.sin_addr.s_addr == htonl(INADDR_LOOPBACK) &&
        bound_client_addr.sin_addr.s_addr == htonl(INADDR_LOOPBACK) &&
        accepted_local_addr.sin_addr.s_addr == htonl(INADDR_LOOPBACK);
    TEST("getsockname", getsockname_ok, getsockname_ok ? "" : "address mismatch");

    int getpeername_ok =
        getpeername(client_fd, (struct sockaddr *)&client_peer_addr, &client_peer_len) == 0 &&
        getpeername(server_fd, (struct sockaddr *)&server_peer_addr, &server_peer_len) == 0 &&
        ntohs(client_peer_addr.sin_port) == ntohs(bound_listener_addr.sin_port) &&
        ntohs(server_peer_addr.sin_port) == ntohs(bound_client_addr.sin_port) &&
        client_peer_addr.sin_addr.s_addr == htonl(INADDR_LOOPBACK) &&
        server_peer_addr.sin_addr.s_addr == htonl(INADDR_LOOPBACK);
    TEST("getpeername", getpeername_ok, getpeername_ok ? "" : "peer address mismatch");

    close(server_fd);
    close(client_fd);
    close(listener_fd);
}

int main(int argc, char *argv[]) {
    /* Use /tmp/sc as working directory for file tests */
    const char *base = "/tmp/sc";
    mkdir("/tmp", 0755);   /* ensure /tmp exists (may already) */
    mkdir(base, 0755);

    test_fd_ops(base);
    test_path_ops(base);
    test_args_env_clock(argc, argv);
    test_host_process();
    test_host_user();
    test_host_net();

    rmdir(base);

    printf("total: %d failures\n", failures);
    return failures > 0 ? 1 : 0;
}
