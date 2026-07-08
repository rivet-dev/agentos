/* fwrite.c — write argv[2] content to file at argv[1] */
#include <stdio.h>
#include <string.h>

int main(int argc, char *argv[]) {
    if (argc < 3) {
        fprintf(stderr, "usage: fwrite <file> <content>\n");
        return 1;
    }

    FILE *f = fopen(argv[1], "w");
    if (!f) {
        perror(argv[1]);
        return 1;
    }

    fwrite(argv[2], 1, strlen(argv[2]), f);
    fwrite("\n", 1, 1, f);
    fclose(f);
    return 0;
}
