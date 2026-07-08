/* cat.c — reads stdin and writes to stdout */
#include <stdio.h>

int main(void) {
    char buf[4096];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), stdin)) > 0) {
        fwrite(buf, 1, n, stdout);
    }
    return 0;
}
