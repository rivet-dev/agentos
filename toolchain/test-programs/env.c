/* env.c — prints all environment variables */
#include <stdio.h>

extern char **environ;

int main(void) {
    for (char **ep = environ; *ep != NULL; ep++) {
        printf("%s\n", *ep);
    }
    return 0;
}
