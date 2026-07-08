/* unzip.c — Extract ZIP archives using zlib/minizip
 *
 * Usage: unzip archive.zip                 (extract all to cwd)
 *        unzip -d outdir archive.zip       (extract to directory)
 *        unzip -l archive.zip              (list contents)
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <errno.h>
#include <stdint.h>
#include <fcntl.h>
#include <unistd.h>
#include "ioapi.h"
#include "unzip.h"

#define MAX_PATH_LEN 4096
#define WRITE_BUF_SIZE 8192

/* Cap per-entry allocation in the fallback parser. Hostile central directory
 * records can claim sizes up to 4 GiB; refuse anything above this bound. */
#define MAX_UNCOMPRESSED_SIZE (256u * 1024u * 1024u)

typedef struct {
    FILE *file;
    char *filename;
    char mode[4];
    long position;
    long size;
} unzip_file_stream;

static voidpf ZCALLBACK unzip_open_file(voidpf opaque, const char *filename, int mode) {
    unzip_file_stream *stream = NULL;
    const char *mode_fopen = NULL;
    (void)opaque;

    if ((mode & ZLIB_FILEFUNC_MODE_READWRITEFILTER) == ZLIB_FILEFUNC_MODE_READ)
        mode_fopen = "rb";
    else if (mode & ZLIB_FILEFUNC_MODE_EXISTING)
        mode_fopen = "r+b";
    else if (mode & ZLIB_FILEFUNC_MODE_CREATE)
        mode_fopen = "wb";

    if (filename == NULL || mode_fopen == NULL)
        return NULL;

    stream = (unzip_file_stream *)calloc(1, sizeof(unzip_file_stream));
    if (!stream)
        return NULL;

    stream->filename = (char *)malloc(strlen(filename) + 1);
    if (!stream->filename) {
        free(stream);
        return NULL;
    }
    strcpy(stream->filename, filename);
    strncpy(stream->mode, mode_fopen, sizeof(stream->mode) - 1);
    stream->mode[sizeof(stream->mode) - 1] = '\0';

    stream->file = fopen(filename, mode_fopen);
    if (!stream->file) {
        free(stream->filename);
        free(stream);
        return NULL;
    }
    struct stat st;
    stream->size = stat(filename, &st) == 0 ? (long)st.st_size : 0;
    stream->position = 0;
    return stream;
}

static uLong ZCALLBACK unzip_read_file(voidpf opaque, voidpf stream, void *buf, uLong size) {
    unzip_file_stream *file_stream = (unzip_file_stream *)stream;
    uLong got;
    (void)opaque;
    got = (uLong)fread(buf, 1, (size_t)size, file_stream->file);
    file_stream->position += (long)got;
    return got;
}

static uLong ZCALLBACK unzip_write_file(voidpf opaque, voidpf stream, const void *buf, uLong size) {
    unzip_file_stream *file_stream = (unzip_file_stream *)stream;
    uLong wrote;
    (void)opaque;
    wrote = (uLong)fwrite(buf, 1, (size_t)size, file_stream->file);
    file_stream->position += (long)wrote;
    if (file_stream->position > file_stream->size)
        file_stream->size = file_stream->position;
    return wrote;
}

static long ZCALLBACK unzip_tell_file(voidpf opaque, voidpf stream) {
    unzip_file_stream *file_stream = (unzip_file_stream *)stream;
    (void)opaque;
    return file_stream->position;
}

static long ZCALLBACK unzip_seek_file(voidpf opaque, voidpf stream, uLong offset, int origin) {
    int fseek_origin = 0;
    long seek_offset = (long)offset;
    unzip_file_stream *file_stream = (unzip_file_stream *)stream;
    (void)opaque;

    switch (origin) {
    case ZLIB_FILEFUNC_SEEK_CUR:
        seek_offset = file_stream->position + (long)offset;
        fseek_origin = SEEK_SET;
        break;
    case ZLIB_FILEFUNC_SEEK_END:
        seek_offset = file_stream->size + (long)offset;
        fseek_origin = SEEK_SET;
        break;
    case ZLIB_FILEFUNC_SEEK_SET:
        fseek_origin = SEEK_SET;
        break;
    default:
        return -1;
    }

    fclose(file_stream->file);
    file_stream->file = fopen(file_stream->filename, file_stream->mode);
    if (!file_stream->file)
        return -1;

    if (fseek(file_stream->file, seek_offset, fseek_origin) != 0)
        return -1;
    clearerr(file_stream->file);
    file_stream->position = seek_offset;
    return 0;
}

static int ZCALLBACK unzip_close_file(voidpf opaque, voidpf stream) {
    unzip_file_stream *file_stream = (unzip_file_stream *)stream;
    int ret;
    (void)opaque;
    ret = fclose(file_stream->file);
    free(file_stream->filename);
    free(file_stream);
    return ret;
}

static int ZCALLBACK unzip_error_file(voidpf opaque, voidpf stream) {
    unzip_file_stream *file_stream = (unzip_file_stream *)stream;
    (void)opaque;
    return ferror(file_stream->file);
}

static unzFile open_archive(const char *archive) {
    zlib_filefunc_def filefunc = {
        .zopen_file = unzip_open_file,
        .zread_file = unzip_read_file,
        .zwrite_file = unzip_write_file,
        .ztell_file = unzip_tell_file,
        .zseek_file = unzip_seek_file,
        .zclose_file = unzip_close_file,
        .zerror_file = unzip_error_file,
        .opaque = NULL,
    };
    return unzOpen2(archive, &filefunc);
}

/* Ensure all parent directories of path exist */
static int mkdirs(const char *path) {
    char tmp[MAX_PATH_LEN];
    size_t len = strlen(path);
    if (len >= sizeof(tmp)) return -1;
    memcpy(tmp, path, len + 1);

    for (size_t i = 1; i < len; i++) {
        if (tmp[i] == '/') {
            tmp[i] = '\0';
            if (mkdir(tmp, 0755) != 0 && errno != EEXIST)
                return -1;
            tmp[i] = '/';
        }
    }
    return 0;
}

static uint16_t read_le16(const unsigned char *p) {
    return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}

static uint32_t read_le32(const unsigned char *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static int read_archive_bytes(const char *archive, unsigned char **out, size_t *out_len) {
    FILE *f = fopen(archive, "rb");
    long size;
    unsigned char *data;
    if (!f)
        return -1;
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return -1;
    }
    size = ftell(f);
    if (size < 0 || fseek(f, 0, SEEK_SET) != 0) {
        fclose(f);
        return -1;
    }
    data = (unsigned char *)malloc((size_t)size);
    if (!data) {
        fclose(f);
        return -1;
    }
    if (fread(data, 1, (size_t)size, f) != (size_t)size) {
        free(data);
        fclose(f);
        return -1;
    }
    fclose(f);
    *out = data;
    *out_len = (size_t)size;
    return 0;
}

static int find_eocd(const unsigned char *data, size_t len, size_t *eocd_offset) {
    size_t min = len > 0xffff + 22 ? len - (0xffff + 22) : 0;
    if (len < 22)
        return -1;
    for (size_t pos = len - 22; pos + 4 <= len && pos >= min; pos--) {
        if (read_le32(data + pos) == 0x06054b50) {
            *eocd_offset = pos;
            return 0;
        }
        if (pos == 0)
            break;
    }
    return -1;
}

static const char *entry_output_name(const char *name, size_t name_len) {
    const char *end = name + name_len;
    while (name < end && *name == '/')
        name++;
    return name;
}

static int inflate_raw_entry(const unsigned char *src, size_t src_len, unsigned char *dst, size_t dst_len) {
    z_stream stream;
    memset(&stream, 0, sizeof(stream));
    stream.next_in = (Bytef *)src;
    stream.avail_in = (uInt)src_len;
    stream.next_out = dst;
    stream.avail_out = (uInt)dst_len;
    if (inflateInit2(&stream, -MAX_WBITS) != Z_OK)
        return -1;
    int result = inflate(&stream, Z_FINISH);
    inflateEnd(&stream);
    return result == Z_STREAM_END && stream.total_out == dst_len ? 0 : -1;
}

static int simple_archive_entries(const unsigned char *data, size_t len, size_t *cd_offset, uint16_t *entry_count) {
    size_t eocd;
    if (len < 22 || find_eocd(data, len, &eocd) != 0 || eocd > len - 22)
        return -1;
    *entry_count = read_le16(data + eocd + 10);
    *cd_offset = read_le32(data + eocd + 16);
    return *cd_offset < len ? 0 : -1;
}

static int simple_list_archive(const char *archive) {
    unsigned char *data = NULL;
    size_t len = 0;
    size_t pos;
    uint16_t entries;
    unsigned long total_size = 0;
    if (read_archive_bytes(archive, &data, &len) != 0 ||
        simple_archive_entries(data, len, &pos, &entries) != 0) {
        free(data);
        return 1;
    }

    printf("  Length      Name\n");
    printf("---------  ----\n");
    for (uint16_t i = 0; i < entries; i++) {
        uint16_t name_len;
        uint16_t extra_len;
        uint16_t comment_len;
        uint32_t uncompressed_size;
        if (len < 46 || pos > len - 46 || read_le32(data + pos) != 0x02014b50) {
            free(data);
            return 1;
        }
        uncompressed_size = read_le32(data + pos + 24);
        name_len = read_le16(data + pos + 28);
        extra_len = read_le16(data + pos + 30);
        comment_len = read_le16(data + pos + 32);
        size_t header_len = 46 + (size_t)name_len + (size_t)extra_len + (size_t)comment_len;
        if (header_len > len - pos) {
            free(data);
            return 1;
        }
        printf("%9lu  %.*s\n", (unsigned long)uncompressed_size, name_len, data + pos + 46);
        total_size += uncompressed_size;
        pos += header_len;
    }
    printf("---------  ----\n");
    printf("%9lu  %u file(s)\n", total_size, entries);
    free(data);
    return 0;
}

static int simple_extract_archive(const char *archive, const char *outdir) {
    unsigned char *data = NULL;
    size_t len = 0;
    size_t pos;
    uint16_t entries;
    int errors = 0;
    if (read_archive_bytes(archive, &data, &len) != 0 ||
        simple_archive_entries(data, len, &pos, &entries) != 0) {
        free(data);
        return 1;
    }

    if (outdir && mkdir(outdir, 0755) != 0 && errno != EEXIST) {
        fprintf(stderr, "unzip: cannot create directory '%s': %s\n", outdir, strerror(errno));
        free(data);
        return 1;
    }

    for (uint16_t i = 0; i < entries; i++) {
        uint16_t method;
        uint16_t name_len;
        uint16_t extra_len;
        uint16_t comment_len;
        uint16_t local_name_len;
        uint16_t local_extra_len;
        uint32_t compressed_size;
        uint32_t uncompressed_size;
        uint32_t local_offset;
        size_t file_data_offset;
        const char *name;
        const char *safe_name;
        char outpath[MAX_PATH_LEN];
        unsigned char *out = NULL;

        if (len < 46 || pos > len - 46 || read_le32(data + pos) != 0x02014b50) {
            errors++;
            break;
        }
        method = read_le16(data + pos + 10);
        compressed_size = read_le32(data + pos + 20);
        uncompressed_size = read_le32(data + pos + 24);
        name_len = read_le16(data + pos + 28);
        extra_len = read_le16(data + pos + 30);
        comment_len = read_le16(data + pos + 32);
        local_offset = read_le32(data + pos + 42);
        size_t header_len = 46 + (size_t)name_len + (size_t)extra_len + (size_t)comment_len;
        if (header_len > len - pos || (size_t)local_offset > len - 30) {
            errors++;
            break;
        }

        name = (const char *)(data + pos + 46);
        safe_name = entry_output_name(name, name_len);
        size_t safe_len = (size_t)name_len - (size_t)(safe_name - name);
        pos += header_len;
        if (safe_len == 0)
            continue;
        snprintf(outpath, sizeof(outpath), "%s%s%.*s",
                 outdir ? outdir : "", outdir ? "/" : "", (int)safe_len, safe_name);

        size_t out_len = strlen(outpath);
        if (out_len > 0 && outpath[out_len - 1] == '/') {
            if (mkdir(outpath, 0755) != 0 && errno != EEXIST)
                errors++;
            continue;
        }
        if (mkdirs(outpath) != 0) {
            errors++;
            continue;
        }

        if (read_le32(data + local_offset) != 0x04034b50) {
            errors++;
            continue;
        }
        local_name_len = read_le16(data + local_offset + 26);
        local_extra_len = read_le16(data + local_offset + 28);
        size_t local_header_len = 30 + (size_t)local_name_len + (size_t)local_extra_len;
        if (local_header_len > len - (size_t)local_offset) {
            errors++;
            continue;
        }
        file_data_offset = (size_t)local_offset + local_header_len;
        if ((size_t)compressed_size > len - file_data_offset) {
            errors++;
            continue;
        }

        if (uncompressed_size > MAX_UNCOMPRESSED_SIZE) {
            fprintf(stderr, "unzip: entry '%.*s' too large (%lu bytes)\n",
                    (int)safe_len, safe_name, (unsigned long)uncompressed_size);
            errors++;
            continue;
        }
        out = (unsigned char *)malloc(uncompressed_size > 0 ? uncompressed_size : 1);
        if (!out) {
            errors++;
            continue;
        }
        if (method == 0) {
            if (compressed_size != uncompressed_size) {
                errors++;
                free(out);
                continue;
            }
            memcpy(out, data + file_data_offset, uncompressed_size);
        } else if (method == Z_DEFLATED) {
            if (inflate_raw_entry(data + file_data_offset, compressed_size, out, uncompressed_size) != 0) {
                errors++;
                free(out);
                continue;
            }
        } else {
            fprintf(stderr, "unzip: unsupported compression method %u for '%.*s'\n", method, name_len, name);
            errors++;
            free(out);
            continue;
        }

        int fd = open(outpath, O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (fd < 0) {
            fprintf(stderr, "unzip: cannot create '%s': %s\n", outpath, strerror(errno));
            errors++;
            free(out);
            continue;
        }
        size_t written = 0;
        while (written < uncompressed_size) {
            ssize_t n = write(fd, out + written, uncompressed_size - written);
            if (n <= 0) {
                errors++;
                break;
            }
            written += (size_t)n;
        }
        close(fd);
        free(out);
    }

    free(data);
    if (errors > 0) {
        fprintf(stderr, "unzip: completed with %d error(s)\n", errors);
        return 1;
    }
    return 0;
}

/* List archive contents */
static int list_archive(const char *archive) {
    unzFile uf = open_archive(archive);
    if (!uf) {
        return simple_list_archive(archive);
    }

    unz_global_info gi;
    if (unzGetGlobalInfo(uf, &gi) != UNZ_OK) {
        fprintf(stderr, "unzip: cannot read archive info\n");
        unzClose(uf);
        return 1;
    }

    printf("  Length      Name\n");
    printf("---------  ----\n");

    unsigned long total_size = 0;
    for (uLong i = 0; i < gi.number_entry; i++) {
        char filename[MAX_PATH_LEN];
        unz_file_info fi;
        if (unzGetCurrentFileInfo(uf, &fi, filename, sizeof(filename),
                                  NULL, 0, NULL, 0) != UNZ_OK) {
            fprintf(stderr, "unzip: error reading file info\n");
            unzClose(uf);
            return 1;
        }

        printf("%9lu  %s\n", fi.uncompressed_size, filename);
        total_size += fi.uncompressed_size;

        if (i + 1 < gi.number_entry) {
            if (unzGoToNextFile(uf) != UNZ_OK) {
                fprintf(stderr, "unzip: error iterating archive\n");
                unzClose(uf);
                return 1;
            }
        }
    }

    printf("---------  ----\n");
    printf("%9lu  %lu file(s)\n", total_size, gi.number_entry);

    unzClose(uf);
    return 0;
}

/* Extract a single file from the archive */
static int extract_current_file(unzFile uf, const char *outdir) {
    char filename[MAX_PATH_LEN];
    unz_file_info fi;
    if (unzGetCurrentFileInfo(uf, &fi, filename, sizeof(filename),
                              NULL, 0, NULL, 0) != UNZ_OK) {
        fprintf(stderr, "unzip: error reading file info\n");
        return -1;
    }

    /* Build output path */
    char outpath[MAX_PATH_LEN];
    if (outdir) {
        snprintf(outpath, sizeof(outpath), "%s/%s", outdir, filename);
    } else {
        snprintf(outpath, sizeof(outpath), "%s", filename);
    }

    /* Directory entry (trailing slash) */
    size_t namelen = strlen(outpath);
    if (namelen > 0 && outpath[namelen - 1] == '/') {
        if (mkdir(outpath, 0755) != 0 && errno != EEXIST) {
            fprintf(stderr, "unzip: cannot create directory '%s': %s\n",
                    outpath, strerror(errno));
            return -1;
        }
        return 0;
    }

    /* Ensure parent directory exists */
    if (mkdirs(outpath) != 0) {
        fprintf(stderr, "unzip: cannot create parent directories for '%s'\n", outpath);
        return -1;
    }

    if (unzOpenCurrentFile(uf) != UNZ_OK) {
        fprintf(stderr, "unzip: cannot open '%s' in archive\n", filename);
        return -1;
    }

    FILE *fout = fopen(outpath, "wb");
    if (!fout) {
        fprintf(stderr, "unzip: cannot create '%s': %s\n", outpath, strerror(errno));
        unzCloseCurrentFile(uf);
        return -1;
    }

    unsigned char buf[WRITE_BUF_SIZE];
    int err = UNZ_OK;
    int bytes;
    while ((bytes = unzReadCurrentFile(uf, buf, sizeof(buf))) > 0) {
        if (fwrite(buf, 1, (size_t)bytes, fout) != (size_t)bytes) {
            fprintf(stderr, "unzip: error writing '%s'\n", outpath);
            err = -1;
            break;
        }
    }
    if (bytes < 0) {
        fprintf(stderr, "unzip: error reading '%s' from archive\n", filename);
        err = -1;
    }

    fclose(fout);
    unzCloseCurrentFile(uf);
    return err;
}

/* Extract all files from the archive */
static int extract_archive(const char *archive, const char *outdir) {
    unzFile uf = open_archive(archive);
    if (!uf) {
        return simple_extract_archive(archive, outdir);
    }

    /* Create output directory if specified */
    if (outdir) {
        if (mkdir(outdir, 0755) != 0 && errno != EEXIST) {
            fprintf(stderr, "unzip: cannot create directory '%s': %s\n",
                    outdir, strerror(errno));
            unzClose(uf);
            return 1;
        }
    }

    unz_global_info gi;
    if (unzGetGlobalInfo(uf, &gi) != UNZ_OK) {
        fprintf(stderr, "unzip: cannot read archive info\n");
        unzClose(uf);
        return 1;
    }

    int errors = 0;
    for (uLong i = 0; i < gi.number_entry; i++) {
        if (extract_current_file(uf, outdir) != 0)
            errors++;

        if (i + 1 < gi.number_entry) {
            if (unzGoToNextFile(uf) != UNZ_OK) {
                fprintf(stderr, "unzip: error iterating archive\n");
                unzClose(uf);
                return 1;
            }
        }
    }

    unzClose(uf);

    if (errors > 0) {
        fprintf(stderr, "unzip: completed with %d error(s)\n", errors);
        return 1;
    }
    return 0;
}

static void print_usage(void) {
    fprintf(stderr, "Usage: unzip [-l] [-d dir] archive.zip\n");
    fprintf(stderr, "  -l       List archive contents\n");
    fprintf(stderr, "  -d dir   Extract to directory\n");
}

int main(int argc, char *argv[]) {
    if (argc < 2) {
        print_usage();
        return 1;
    }

    int list_mode = 0;
    const char *outdir = NULL;
    const char *archive = NULL;
    int i = 1;

    /* Parse flags */
    while (i < argc && argv[i][0] == '-') {
        if (strcmp(argv[i], "-l") == 0) {
            list_mode = 1;
            i++;
        } else if (strcmp(argv[i], "-d") == 0) {
            if (i + 1 >= argc) {
                fprintf(stderr, "unzip: -d requires a directory argument\n");
                return 1;
            }
            outdir = argv[i + 1];
            i += 2;
        } else if (strcmp(argv[i], "--") == 0) {
            i++;
            break;
        } else {
            fprintf(stderr, "unzip: unknown option '%s'\n", argv[i]);
            print_usage();
            return 1;
        }
    }

    if (i >= argc) {
        fprintf(stderr, "unzip: no archive specified\n");
        print_usage();
        return 1;
    }

    archive = argv[i];

    if (list_mode) {
        return list_archive(archive);
    } else {
        return extract_archive(archive, outdir);
    }
}
