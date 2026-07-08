/* sort.c — sort lines from stdin alphabetically */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int cmp(const void *a, const void *b) {
    return strcmp(*(const char **)a, *(const char **)b);
}

int main(void) {
    char **lines = NULL;
    size_t count = 0, cap = 0;
    char buf[4096];

    while (fgets(buf, sizeof(buf), stdin)) {
        if (count >= cap) {
            cap = cap ? cap * 2 : 64;
            lines = realloc(lines, cap * sizeof(char *));
        }
        lines[count++] = strdup(buf);
    }

    qsort(lines, count, sizeof(char *), cmp);

    for (size_t i = 0; i < count; i++) {
        fputs(lines[i], stdout);
        free(lines[i]);
    }
    free(lines);
    return 0;
}
