/* c-find.c — find files matching a pattern (recursive + string matching) */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>

static int cmpstr(const void *a, const void *b) {
    return strcmp(*(const char **)a, *(const char **)b);
}

/* Simple glob matching: * matches any sequence, ? matches one char */
static int glob_match(const char *pat, const char *str) {
    while (*pat) {
        if (*pat == '*') {
            pat++;
            if (!*pat) return 1;
            while (*str) {
                if (glob_match(pat, str)) return 1;
                str++;
            }
            return 0;
        }
        if (*pat == '?' || *pat == *str) {
            pat++;
            str++;
        } else {
            return 0;
        }
    }
    return *str == 0;
}

static void find_in(const char *path, const char *pattern) {
    DIR *dir = opendir(path);
    if (!dir) return;

    char *names[1024];
    int count = 0;
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL && count < 1024) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0)
            continue;
        names[count++] = strdup(ent->d_name);
    }
    closedir(dir);

    qsort(names, count, sizeof(char *), cmpstr);

    for (int i = 0; i < count; i++) {
        char full[4096];
        snprintf(full, sizeof(full), "%s/%s", path, names[i]);

        if (glob_match(pattern, names[i])) {
            printf("%s\n", full);
        }

        struct stat st;
        if (stat(full, &st) == 0 && S_ISDIR(st.st_mode)) {
            find_in(full, pattern);
        }
        free(names[i]);
    }
}

int main(int argc, char *argv[]) {
    if (argc < 3) {
        fprintf(stderr, "usage: c-find <dir> <pattern>\n");
        return 1;
    }
    find_in(argv[1], argv[2]);
    return 0;
}
