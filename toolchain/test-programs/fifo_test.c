#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static const char payload[] = "fifo";

static int run_reader(const char *path) {
    int reader = open(path, O_RDONLY);
    if (reader < 0) {
        perror("open blocking FIFO reader");
        return 1;
    }
    char received[sizeof(payload)] = {0};
    if (read(reader, received, sizeof(received) - 1) != sizeof(payload) - 1 ||
        memcmp(received, payload, sizeof(payload) - 1) != 0) {
        fprintf(stderr, "blocking FIFO payload mismatch\n");
        close(reader);
        return 1;
    }
    close(reader);
    puts("fifo-reader-ok");
    return 0;
}

static int run_writer(const char *path) {
    int writer = open(path, O_WRONLY);
    if (writer < 0) {
        perror("open blocking FIFO writer");
        return 1;
    }
    if (write(writer, payload, sizeof(payload) - 1) != sizeof(payload) - 1) {
        perror("write blocking FIFO");
        close(writer);
        return 1;
    }
    close(writer);
    puts("fifo-writer-ok");
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--reader") == 0) {
        return run_reader(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "--writer") == 0) {
        return run_writer(argv[2]);
    }
    if (argc != 2) {
        fprintf(stderr, "usage: fifo_test [--reader|--writer] PATH\n");
        return 2;
    }

    int reader = open(argv[1], O_RDONLY | O_NONBLOCK);
    if (reader < 0) {
        perror("open FIFO reader");
        return 1;
    }
    int writer = open(argv[1], O_WRONLY | O_NONBLOCK);
    if (writer < 0) {
        perror("open FIFO writer");
        close(reader);
        return 1;
    }

    if (write(writer, payload, sizeof(payload) - 1) != sizeof(payload) - 1) {
        perror("write FIFO");
        close(writer);
        close(reader);
        return 1;
    }
    char received[sizeof(payload)] = {0};
    if (read(reader, received, sizeof(received) - 1) != sizeof(payload) - 1 ||
        memcmp(received, payload, sizeof(payload) - 1) != 0) {
        fprintf(stderr, "FIFO payload mismatch\n");
        close(writer);
        close(reader);
        return 1;
    }
    close(writer);
    close(reader);

    errno = 0;
    writer = open(argv[1], O_WRONLY | O_NONBLOCK);
    if (writer >= 0 || errno != ENXIO) {
        fprintf(stderr, "writer without reader: expected ENXIO, got %s\n",
                writer >= 0 ? "success" : strerror(errno));
        if (writer >= 0) close(writer);
        return 1;
    }

    puts("fifo-nonblocking-ok");
    return 0;
}
