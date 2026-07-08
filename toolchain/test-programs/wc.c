/* wc.c — word/line/byte count from stdin */
#include <stdio.h>
#include <ctype.h>

int main(void) {
    int c;
    long lines = 0, words = 0, bytes = 0;
    int in_word = 0;

    while ((c = getchar()) != EOF) {
        bytes++;
        if (c == '\n') lines++;
        if (isspace(c)) {
            in_word = 0;
        } else if (!in_word) {
            in_word = 1;
            words++;
        }
    }

    printf("%ld %ld %ld\n", lines, words, bytes);
    return 0;
}
