/* c-tree.c — recursive directory listing */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>

struct entry {
    char *name;
    int isdir;
};

static int cmp_entry(const void *a, const void *b) {
    return strcmp(((const struct entry *)a)->name, ((const struct entry *)b)->name);
}

static void tree(const char *path, const char *prefix) {
    DIR *dir = opendir(path);
    if (!dir) return;

    struct entry entries[1024];
    int count = 0;
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL && count < 1024) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0)
            continue;
        entries[count].name = strdup(ent->d_name);
        char full[4096];
        snprintf(full, sizeof(full), "%s/%s", path, ent->d_name);
        struct stat st;
        entries[count].isdir = (stat(full, &st) == 0 && S_ISDIR(st.st_mode));
        count++;
    }
    closedir(dir);

    qsort(entries, count, sizeof(struct entry), cmp_entry);

    for (int i = 0; i < count; i++) {
        int last = (i == count - 1);
        printf("%s%s %s\n", prefix, last ? "`--" : "|--", entries[i].name);
        if (entries[i].isdir) {
            char newpfx[4096];
            snprintf(newpfx, sizeof(newpfx), "%s%s", prefix, last ? "    " : "|   ");
            char full[4096];
            snprintf(full, sizeof(full), "%s/%s", path, entries[i].name);
            tree(full, newpfx);
        }
        free(entries[i].name);
    }
}

int main(int argc, char *argv[]) {
    const char *path = argc > 1 ? argv[1] : ".";
    printf("%s\n", path);
    tree(path, "");
    return 0;
}
