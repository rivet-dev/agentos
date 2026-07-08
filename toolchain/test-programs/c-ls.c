/* c-ls.c — list directory contents with file sizes */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>

static int cmpstr(const void *a, const void *b) {
    return strcmp(*(const char **)a, *(const char **)b);
}

int main(int argc, char *argv[]) {
    const char *path = argc > 1 ? argv[1] : ".";

    DIR *dir = opendir(path);
    if (!dir) {
        perror(path);
        return 1;
    }

    /* Collect entry names */
    char *names[1024];
    int count = 0;
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL && count < 1024) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0)
            continue;
        names[count++] = strdup(ent->d_name);
    }
    closedir(dir);

    /* Sort for deterministic output */
    qsort(names, count, sizeof(char *), cmpstr);

    for (int i = 0; i < count; i++) {
        char full[4096];
        snprintf(full, sizeof(full), "%s/%s", path, names[i]);
        struct stat st;
        if (stat(full, &st) == 0) {
            if (S_ISDIR(st.st_mode)) {
                printf("d %s\n", names[i]);
            } else {
                printf("- %lld %s\n", (long long)st.st_size, names[i]);
            }
        }
        free(names[i]);
    }
    return 0;
}
