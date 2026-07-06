/* c-cp.c — copy a file (open + read + write + close) */
#include <stdio.h>

int main(int argc, char *argv[]) {
    if (argc < 3) {
        fprintf(stderr, "usage: c-cp <source> <dest>\n");
        return 1;
    }

    FILE *src = fopen(argv[1], "rb");
    if (!src) {
        perror(argv[1]);
        return 1;
    }

    FILE *dst = fopen(argv[2], "wb");
    if (!dst) {
        perror(argv[2]);
        fclose(src);
        return 1;
    }

    char buf[4096];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), src)) > 0) {
        fwrite(buf, 1, n, dst);
    }

    fclose(src);
    fclose(dst);
    printf("copied: %s -> %s\n", argv[1], argv[2]);
    return 0;
}
