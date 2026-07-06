/* zip.c — Create ZIP archives using zlib/minizip
 *
 * Usage: zip [-r] archive.zip file1 [file2 ...]
 *   -r   Recurse into directories
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <dirent.h>
#include <time.h>
#include "zip.h"

#define MAX_PATH_LEN 4096
#define READ_BUF_SIZE 8192

/* Add a single file to the zip archive */
static int add_file_to_zip(zipFile zf, const char *filepath, const char *archivepath) {
    FILE *fin = fopen(filepath, "rb");
    if (!fin) {
        fprintf(stderr, "zip: cannot open '%s': ", filepath);
        perror("");
        return -1;
    }

    zip_fileinfo zi;
    memset(&zi, 0, sizeof(zi));

    /* Get file modification time */
    struct stat st;
    if (stat(filepath, &st) == 0) {
        struct tm *lt = localtime(&st.st_mtime);
        if (lt) {
            zi.tmz_date.tm_sec  = lt->tm_sec;
            zi.tmz_date.tm_min  = lt->tm_min;
            zi.tmz_date.tm_hour = lt->tm_hour;
            zi.tmz_date.tm_mday = lt->tm_mday;
            zi.tmz_date.tm_mon  = lt->tm_mon;
            zi.tmz_date.tm_year = lt->tm_year;
        }
    }

    int err = zipOpenNewFileInZip(zf, archivepath, &zi,
        NULL, 0, NULL, 0, NULL,
        Z_DEFLATED, Z_DEFAULT_COMPRESSION);
    if (err != ZIP_OK) {
        fprintf(stderr, "zip: error opening '%s' in archive\n", archivepath);
        fclose(fin);
        return -1;
    }

    unsigned char buf[READ_BUF_SIZE];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), fin)) > 0) {
        if (zipWriteInFileInZip(zf, buf, (unsigned int)n) != ZIP_OK) {
            fprintf(stderr, "zip: error writing '%s' to archive\n", archivepath);
            fclose(fin);
            zipCloseFileInZip(zf);
            return -1;
        }
    }

    fclose(fin);
    zipCloseFileInZip(zf);
    return 0;
}

/* Recursively add directory contents to zip */
static int add_dir_to_zip(zipFile zf, const char *dirpath, const char *archivebase) {
    DIR *d = opendir(dirpath);
    if (!d) {
        fprintf(stderr, "zip: cannot open directory '%s': ", dirpath);
        perror("");
        return -1;
    }

    /* Add directory entry (trailing slash) */
    char direntry[MAX_PATH_LEN];
    snprintf(direntry, sizeof(direntry), "%s/", archivebase);
    zip_fileinfo zi;
    memset(&zi, 0, sizeof(zi));
    zipOpenNewFileInZip(zf, direntry, &zi, NULL, 0, NULL, 0, NULL, 0, 0);
    zipCloseFileInZip(zf);

    struct dirent *entry;
    int err = 0;
    while ((entry = readdir(d)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
            continue;

        char fullpath[MAX_PATH_LEN];
        char arcpath[MAX_PATH_LEN];
        snprintf(fullpath, sizeof(fullpath), "%s/%s", dirpath, entry->d_name);
        snprintf(arcpath, sizeof(arcpath), "%s/%s", archivebase, entry->d_name);

        struct stat st;
        if (stat(fullpath, &st) != 0) {
            fprintf(stderr, "zip: cannot stat '%s': ", fullpath);
            perror("");
            err = -1;
            continue;
        }

        if (S_ISDIR(st.st_mode)) {
            if (add_dir_to_zip(zf, fullpath, arcpath) != 0)
                err = -1;
        } else {
            if (add_file_to_zip(zf, fullpath, arcpath) != 0)
                err = -1;
        }
    }

    closedir(d);
    return err;
}

static void print_usage(void) {
    fprintf(stderr, "Usage: zip [-r] archive.zip file1 [file2 ...]\n");
    fprintf(stderr, "  -r   Recurse into directories\n");
}

int main(int argc, char *argv[]) {
    if (argc < 3) {
        print_usage();
        return 1;
    }

    int recursive = 0;
    int arg_start = 1;

    /* Parse flags */
    while (arg_start < argc && argv[arg_start][0] == '-') {
        if (strcmp(argv[arg_start], "-r") == 0) {
            recursive = 1;
            arg_start++;
        } else if (strcmp(argv[arg_start], "--") == 0) {
            arg_start++;
            break;
        } else {
            fprintf(stderr, "zip: unknown option '%s'\n", argv[arg_start]);
            print_usage();
            return 1;
        }
    }

    if (argc - arg_start < 2) {
        print_usage();
        return 1;
    }

    const char *archive = argv[arg_start];
    arg_start++;

    zipFile zf = zipOpen(archive, APPEND_STATUS_CREATE);
    if (!zf) {
        fprintf(stderr, "zip: cannot create '%s'\n", archive);
        return 1;
    }

    int errors = 0;
    for (int i = arg_start; i < argc; i++) {
        const char *path = argv[i];

        /* Strip trailing slashes for consistent archive paths */
        char cleanpath[MAX_PATH_LEN];
        strncpy(cleanpath, path, sizeof(cleanpath) - 1);
        cleanpath[sizeof(cleanpath) - 1] = '\0';
        size_t len = strlen(cleanpath);
        while (len > 1 && cleanpath[len - 1] == '/')
            cleanpath[--len] = '\0';

        struct stat st;
        if (stat(cleanpath, &st) != 0) {
            fprintf(stderr, "zip: cannot stat '%s': ", cleanpath);
            perror("");
            errors++;
            continue;
        }

        if (S_ISDIR(st.st_mode)) {
            if (!recursive) {
                fprintf(stderr, "zip: '%s' is a directory (use -r to recurse)\n", cleanpath);
                errors++;
                continue;
            }
            if (add_dir_to_zip(zf, cleanpath, cleanpath) != 0)
                errors++;
        } else {
            if (add_file_to_zip(zf, cleanpath, cleanpath) != 0)
                errors++;
        }
    }

    zipClose(zf, NULL);

    if (errors > 0) {
        fprintf(stderr, "zip: completed with %d error(s)\n", errors);
        return 1;
    }

    return 0;
}
