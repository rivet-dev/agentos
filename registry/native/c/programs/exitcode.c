/* exitcode.c — exits with code from argv[1] */
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char *argv[]) {
    if (argc < 2) {
        fprintf(stderr, "usage: exitcode <code>\n");
        return 1;
    }
    return atoi(argv[1]);
}
