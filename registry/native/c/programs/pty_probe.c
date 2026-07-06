// pty_probe.c — deterministic PTY/stdio probe for AgentOS shell tests.
//
// The program intentionally speaks a tiny line-oriented protocol over stdio so
// integration tests can snapshot terminal state after each PTY operation:
// TTY detection, raw-mode toggling, cursor-position request/response, raw byte
// delivery, cooked Enter behavior, resize notification, and EOF.

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

#if defined(__wasm__)
__attribute__((import_module("host_tty"), import_name("isatty"))) extern unsigned int
host_tty_isatty(unsigned int fd);
__attribute__((import_module("host_tty"), import_name("get_size"))) extern unsigned int
host_tty_get_size(unsigned int fd, unsigned short *cols, unsigned short *rows);
__attribute__((import_module("host_tty"), import_name("set_raw_mode"))) extern unsigned int
host_tty_set_raw_mode(unsigned int enabled);
#else
#include <termios.h>
static struct termios saved_termios;
static int saved_termios_valid = 0;

static unsigned int host_tty_isatty(unsigned int fd) {
    return isatty((int)fd) ? 1u : 0u;
}

static unsigned int host_tty_get_size(unsigned int fd, unsigned short *cols, unsigned short *rows) {
    struct winsize ws;
    memset(&ws, 0, sizeof(ws));
    if (ioctl((int)fd, TIOCGWINSZ, &ws) != 0) {
        return (unsigned int)errno;
    }
    *cols = ws.ws_col;
    *rows = ws.ws_row;
    return 0;
}

static unsigned int host_tty_set_raw_mode(unsigned int enabled) {
    if (enabled) {
        struct termios raw;
        if (tcgetattr(STDIN_FILENO, &saved_termios) != 0) {
            return (unsigned int)errno;
        }
        saved_termios_valid = 1;
        raw = saved_termios;
        cfmakeraw(&raw);
        if (tcsetattr(STDIN_FILENO, TCSANOW, &raw) != 0) {
            return (unsigned int)errno;
        }
        return 0;
    }

    if (saved_termios_valid && tcsetattr(STDIN_FILENO, TCSANOW, &saved_termios) != 0) {
        return (unsigned int)errno;
    }
    return 0;
}
#endif

static void print_hex(const unsigned char *bytes, int len) {
    for (int i = 0; i < len; i++) {
        if (i > 0) {
            fputc(' ', stdout);
        }
        printf("%02X", bytes[i]);
    }
}

static void print_text(const unsigned char *bytes, int len) {
    for (int i = 0; i < len; i++) {
        unsigned char c = bytes[i];
        if (c == '\r') {
            fputs("\\r", stdout);
        } else if (c == '\n') {
            fputs("\\n", stdout);
        } else if (c == '\t') {
            fputs("\\t", stdout);
        } else if (c == 0x1b) {
            fputs("\\e", stdout);
        } else if (c < 0x20 || c == 0x7f) {
            printf("\\x%02X", c);
        } else {
            fputc((int)c, stdout);
        }
    }
}

static int read_until(unsigned char *buf, int cap, unsigned char terminator) {
    int len = 0;
    while (len < cap) {
        unsigned char c = 0;
        ssize_t n = read(STDIN_FILENO, &c, 1);
        if (n == 0) {
            return len == 0 ? 0 : len;
        }
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            printf("READ_ERROR errno=%d\r\n", errno);
            fflush(stdout);
            return -1;
        }
        buf[len++] = c;
        if (c == terminator) {
            return len;
        }
    }
    return len;
}

static void print_size(const char *label) {
    const char *cols_env = getenv("COLUMNS");
    const char *rows_env = getenv("LINES");
    unsigned short host_cols = 0;
    unsigned short host_rows = 0;
    unsigned int host_rc = host_tty_get_size(STDOUT_FILENO, &host_cols, &host_rows);
    struct winsize ws;
    memset(&ws, 0, sizeof(ws));
    errno = 0;
    int rc = ioctl(STDOUT_FILENO, TIOCGWINSZ, &ws);
    printf(
        "%s env_cols=%s env_rows=%s host_rc=%u host_cols=%u host_rows=%u ioctl_rc=%d ioctl_errno=%d ioctl_cols=%u ioctl_rows=%u\r\n",
        label,
        cols_env ? cols_env : "<unset>",
        rows_env ? rows_env : "<unset>",
        host_rc,
        (unsigned int)host_cols,
        (unsigned int)host_rows,
        rc,
        errno,
        (unsigned int)ws.ws_col,
        (unsigned int)ws.ws_row);
    fflush(stdout);
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    printf("PTY_PROBE start\r\n");
    printf(
        "TTY_HOST stdin=%u stdout=%u stderr=%u\r\n",
        host_tty_isatty(STDIN_FILENO),
        host_tty_isatty(STDOUT_FILENO),
        host_tty_isatty(STDERR_FILENO));
    printf(
        "TTY_LIBC stdin=%d stdout=%d stderr=%d\r\n",
        isatty(STDIN_FILENO),
        isatty(STDOUT_FILENO),
        isatty(STDERR_FILENO));
    print_size("SIZE_START");

    unsigned int raw_rc = host_tty_set_raw_mode(1);
    printf("RAW_ON rc=%u\r\n", raw_rc);

    printf("CPR_REQUEST ");
    fflush(stdout);
    write(STDOUT_FILENO, "\x1b[6n", 4);

    unsigned char buf[128];
    int cpr_len = read_until(buf, (int)sizeof(buf), 'R');
    printf("\r\nCPR_REPLY bytes=%d hex=", cpr_len);
    if (cpr_len > 0) {
        print_hex(buf, cpr_len);
    }
    printf(" text=");
    if (cpr_len > 0) {
        print_text(buf, cpr_len);
    }
    printf("\r\n");

    printf("RAW_INPUT> ");
    fflush(stdout);
    int raw_len = read_until(buf, (int)sizeof(buf), '!');
    printf("\r\nRAW_BYTES bytes=%d hex=", raw_len);
    if (raw_len > 0) {
        print_hex(buf, raw_len);
    }
    printf(" text=");
    if (raw_len > 0) {
        print_text(buf, raw_len);
    }
    printf("\r\n");

    unsigned int cooked_rc = host_tty_set_raw_mode(0);
    printf("RAW_OFF rc=%u\r\n", cooked_rc);

    printf("COOKED_INPUT> ");
    fflush(stdout);
    int cooked_len = read_until(buf, (int)sizeof(buf), '\n');
    printf("COOKED_BYTES bytes=%d hex=", cooked_len);
    if (cooked_len > 0) {
        print_hex(buf, cooked_len);
    }
    printf(" text=");
    if (cooked_len > 0) {
        print_text(buf, cooked_len);
    }
    printf("\r\n");

    printf("RESIZE_READY> ");
    fflush(stdout);
    int resize_len = read_until(buf, (int)sizeof(buf), '\n');
    printf("RESIZE_TRIGGER bytes=%d hex=", resize_len);
    if (resize_len > 0) {
        print_hex(buf, resize_len);
    }
    printf(" text=");
    if (resize_len > 0) {
        print_text(buf, resize_len);
    }
    printf("\r\n");
    print_size("SIZE_AFTER_RESIZE");

    printf("EOF_READY> ");
    fflush(stdout);
    unsigned char eof_byte = 0;
    ssize_t eof_read = read(STDIN_FILENO, &eof_byte, 1);
    printf("EOF_READ n=%zd", eof_read);
    if (eof_read > 0) {
        printf(" byte=%02X", eof_byte);
    }
    printf("\r\nPTY_PROBE done\r\n");
    return 0;
}
