#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

static int verify_payload(const char *path, const char *expected) {
    char actual[9] = {0};
    int fd = open(path, O_RDONLY);
    if (fd < 0 || read(fd, actual, 8) != 8 || close(fd) != 0) return -1;
    return memcmp(actual, expected, 8);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: mmap_test FILE\n");
        return 2;
    }
    int fd = open(argv[1], O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (fd < 0 || write(fd, "abcdefgh", 8) != 8) {
        perror("prepare mmap");
        return 1;
    }
    char *shared = mmap(NULL, 8, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (shared == MAP_FAILED || close(fd) != 0) {
        perror("mmap shared");
        return 1;
    }
    memcpy(shared + 2, "XY", 2);
    if (msync(shared, 8, MS_SYNC) != 0 || munmap(shared, 8) != 0 ||
        verify_payload(argv[1], "abXYefgh") != 0) {
        perror("shared writeback");
        return 1;
    }

    fd = open(argv[1], O_RDONLY);
    char *private_map = mmap(NULL, 8, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 0);
    if (private_map == MAP_FAILED || close(fd) != 0) {
        perror("mmap private");
        return 1;
    }
    memcpy(private_map, "PRIVATE!", 8);
    if (munmap(private_map, 8) != 0 || verify_payload(argv[1], "abXYefgh") != 0) {
        perror("private isolation");
        return 1;
    }
    puts("mmap: ok");
    return 0;
}
