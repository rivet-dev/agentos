#include <dirent.h>
#include <errno.h>
#include <grp.h>
#include <limits.h>
#include <pwd.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/xattr.h>
#include <unistd.h>

#define ACL_VERSION 2u
#define ACL_USER_OBJ 0x01u
#define ACL_USER 0x02u
#define ACL_GROUP_OBJ 0x04u
#define ACL_GROUP 0x08u
#define ACL_MASK 0x10u
#define ACL_OTHER 0x20u
#define ACL_UNDEFINED_ID UINT32_MAX
#define ACL_ENTRY_LIMIT 25u
#define ACL_PARSE_ENTRY_LIMIT 4096u

struct acl_entry {
    uint16_t tag;
    uint16_t perm;
    uint32_t id;
};

struct acl {
    struct acl_entry *entries;
    size_t count;
    size_t capacity;
};

static const char *base_name(const char *path) {
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

static void acl_free(struct acl *acl) {
    free(acl->entries);
    memset(acl, 0, sizeof(*acl));
}

static int acl_reserve(struct acl *acl, size_t count) {
    if (count > ACL_PARSE_ENTRY_LIMIT) { errno = E2BIG; return -1; }
    if (count <= acl->capacity) return 0;
    size_t capacity = acl->capacity ? acl->capacity * 2 : 8;
    if (capacity < count) capacity = count;
    if (capacity > ACL_PARSE_ENTRY_LIMIT) capacity = ACL_PARSE_ENTRY_LIMIT;
    struct acl_entry *entries = realloc(acl->entries, capacity * sizeof(*entries));
    if (!entries) return -1;
    acl->entries = entries;
    acl->capacity = capacity;
    return 0;
}

static int acl_add(struct acl *acl, uint16_t tag, uint16_t perm, uint32_t id) {
    if (acl_reserve(acl, acl->count + 1) != 0) return -1;
    acl->entries[acl->count++] = (struct acl_entry){tag, perm, id};
    return 0;
}

static int tag_order(uint16_t tag) {
    switch (tag) {
        case ACL_USER_OBJ: return 0;
        case ACL_USER: return 1;
        case ACL_GROUP_OBJ: return 2;
        case ACL_GROUP: return 3;
        case ACL_MASK: return 4;
        case ACL_OTHER: return 5;
        default: return 6;
    }
}

static int compare_entries(const void *left, const void *right) {
    const struct acl_entry *a = left;
    const struct acl_entry *b = right;
    int order = tag_order(a->tag) - tag_order(b->tag);
    if (order) return order;
    return a->id < b->id ? -1 : a->id > b->id;
}

static void acl_sort(struct acl *acl) {
    qsort(acl->entries, acl->count, sizeof(*acl->entries), compare_entries);
}

static struct acl_entry *acl_find(struct acl *acl, uint16_t tag, uint32_t id) {
    for (size_t i = 0; i < acl->count; i++) {
        if (acl->entries[i].tag == tag && acl->entries[i].id == id) return &acl->entries[i];
    }
    return NULL;
}

static int acl_put(struct acl *acl, uint16_t tag, uint16_t perm, uint32_t id) {
    struct acl_entry *entry = acl_find(acl, tag, id);
    if (entry) { entry->perm = perm; return 0; }
    return acl_add(acl, tag, perm, id);
}

static int acl_remove(struct acl *acl, uint16_t tag, uint32_t id) {
    for (size_t i = 0; i < acl->count; i++) {
        if (acl->entries[i].tag == tag && acl->entries[i].id == id) {
            memmove(&acl->entries[i], &acl->entries[i + 1],
                    (acl->count - i - 1) * sizeof(*acl->entries));
            acl->count--;
            return 0;
        }
    }
    errno = ENODATA;
    return -1;
}

static int acl_from_mode(struct acl *acl, mode_t mode) {
    return acl_add(acl, ACL_USER_OBJ, (mode >> 6) & 7, ACL_UNDEFINED_ID) ||
           acl_add(acl, ACL_GROUP_OBJ, (mode >> 3) & 7, ACL_UNDEFINED_ID) ||
           acl_add(acl, ACL_OTHER, mode & 7, ACL_UNDEFINED_ID);
}

static uint16_t read_u16(const unsigned char *bytes) {
    return (uint16_t)bytes[0] | (uint16_t)bytes[1] << 8;
}

static uint32_t read_u32(const unsigned char *bytes) {
    return (uint32_t)bytes[0] | (uint32_t)bytes[1] << 8 |
           (uint32_t)bytes[2] << 16 | (uint32_t)bytes[3] << 24;
}

static void write_u16(unsigned char *bytes, uint16_t value) {
    bytes[0] = value & 0xff;
    bytes[1] = value >> 8;
}

static void write_u32(unsigned char *bytes, uint32_t value) {
    bytes[0] = value & 0xff;
    bytes[1] = value >> 8;
    bytes[2] = value >> 16;
    bytes[3] = value >> 24;
}

static int acl_decode(const unsigned char *value, size_t size, struct acl *acl) {
    if (size < 4 || (size - 4) % 8 || read_u32(value) != ACL_VERSION) {
        errno = EINVAL;
        return -1;
    }
    size_t count = (size - 4) / 8;
    if (acl_reserve(acl, count) != 0) return -1;
    for (size_t i = 0; i < count; i++) {
        const unsigned char *entry = value + 4 + i * 8;
        acl->entries[i] = (struct acl_entry){
            read_u16(entry), read_u16(entry + 2), read_u32(entry + 4)
        };
    }
    acl->count = count;
    return 0;
}

static unsigned char *acl_encode(struct acl *acl, size_t *size) {
    acl_sort(acl);
    *size = 4 + acl->count * 8;
    unsigned char *value = malloc(*size);
    if (!value) return NULL;
    write_u32(value, ACL_VERSION);
    for (size_t i = 0; i < acl->count; i++) {
        unsigned char *entry = value + 4 + i * 8;
        write_u16(entry, acl->entries[i].tag);
        write_u16(entry + 2, acl->entries[i].perm);
        write_u32(entry + 4, acl->entries[i].id);
    }
    return value;
}

static int read_acl(const char *path, int defaults, int allow_missing, struct acl *acl) {
    const char *name = defaults ? "system.posix_acl_default" : "system.posix_acl_access";
    ssize_t size = getxattr(path, name, NULL, 0);
    if (size < 0) {
        if (errno != ENODATA || !allow_missing) return -1;
        struct stat st;
        if (stat(path, &st) != 0) return -1;
        return acl_from_mode(acl, st.st_mode);
    }

    for (int attempt = 0; attempt < 2; attempt++) {
        size_t capacity = attempt == 0 ? (size_t)size : XATTR_SIZE_MAX;
        unsigned char *value = malloc(capacity + 1);
        if (!value) { errno = ENOMEM; return -1; }
        ssize_t result = getxattr(path, name, value, capacity);
        if (result >= 0) {
            int status = acl_decode(value, (size_t)result, acl);
            free(value);
            return status;
        }

        int saved_errno = errno;
        free(value);
        if (saved_errno != ERANGE || capacity == XATTR_SIZE_MAX) {
            errno = saved_errno;
            return -1;
        }
    }
    errno = ERANGE;
    return -1;
}

static int write_acl(const char *path, int defaults, struct acl *acl) {
    if (acl->count > ACL_ENTRY_LIMIT) { errno = E2BIG; return -1; }
    size_t size = 0;
    unsigned char *value = acl_encode(acl, &size);
    if (!value) return -1;
    const char *name = defaults ? "system.posix_acl_default" : "system.posix_acl_access";
    int result = setxattr(path, name, value, size, 0);
    free(value);
    return result;
}

static int parse_permissions(const char *text, uint16_t *permissions) {
    if (!text || !*text || strlen(text) > 3) return -1;
    uint16_t value = 0;
    for (const char *cursor = text; *cursor; cursor++) {
        if (*cursor == 'r') value |= 4;
        else if (*cursor == 'w') value |= 2;
        else if (*cursor == 'x') value |= 1;
        else if (*cursor != '-') return -1;
    }
    *permissions = value;
    return 0;
}

static void format_permissions(uint16_t permissions, char output[4]) {
    output[0] = permissions & 4 ? 'r' : '-';
    output[1] = permissions & 2 ? 'w' : '-';
    output[2] = permissions & 1 ? 'x' : '-';
    output[3] = 0;
}

static int parse_id(const char *text, int user, uint32_t *id) {
    if (!text || !*text) { *id = ACL_UNDEFINED_ID; return 0; }
    char *end = NULL;
    unsigned long numeric = strtoul(text, &end, 10);
    if (*text && end && !*end && numeric <= UINT32_MAX) {
        *id = (uint32_t)numeric;
        return 0;
    }
    if (user) {
        struct passwd *record = getpwnam(text);
        if (record) { *id = record->pw_uid; return 0; }
    } else {
        struct group *record = getgrnam(text);
        if (record) { *id = record->gr_gid; return 0; }
    }
    return -1;
}

static int parse_entry(char *text, int default_mode, struct acl_entry *entry, int *defaults,
                       int require_permissions) {
    *defaults = default_mode;
    if (!strncmp(text, "default:", 8)) { *defaults = 1; text += 8; }
    else if (!strncmp(text, "d:", 2)) { *defaults = 1; text += 2; }
    char *first = strchr(text, ':');
    if (!first) return -1;
    *first++ = 0;
    char *second = strchr(first, ':');
    char *permissions = NULL;
    if (second) { *second++ = 0; permissions = second; }
    else if (require_permissions &&
             (!strcmp(text, "o") || !strcmp(text, "other") ||
              !strcmp(text, "m") || !strcmp(text, "mask"))) {
        permissions = first;
        first = "";
    } else if (require_permissions) return -1;

    int named = 0;
    if (!strcmp(text, "u") || !strcmp(text, "user")) {
        named = *first != 0;
        entry->tag = named ? ACL_USER : ACL_USER_OBJ;
        if (parse_id(first, 1, &entry->id) != 0) return -1;
    } else if (!strcmp(text, "g") || !strcmp(text, "group")) {
        named = *first != 0;
        entry->tag = named ? ACL_GROUP : ACL_GROUP_OBJ;
        if (parse_id(first, 0, &entry->id) != 0) return -1;
    } else if (!strcmp(text, "m") || !strcmp(text, "mask")) {
        if (*first) return -1;
        entry->tag = ACL_MASK;
        entry->id = ACL_UNDEFINED_ID;
    } else if (!strcmp(text, "o") || !strcmp(text, "other")) {
        if (*first) return -1;
        entry->tag = ACL_OTHER;
        entry->id = ACL_UNDEFINED_ID;
    } else {
        return -1;
    }
    if (!named && (entry->tag == ACL_USER_OBJ || entry->tag == ACL_GROUP_OBJ))
        entry->id = ACL_UNDEFINED_ID;
    if (require_permissions && parse_permissions(permissions, &entry->perm) != 0) return -1;
    return 0;
}

static int has_extended_entries(const struct acl *acl) {
    for (size_t i = 0; i < acl->count; i++) {
        if (acl->entries[i].tag == ACL_USER || acl->entries[i].tag == ACL_GROUP) return 1;
    }
    return 0;
}

static void calculate_mask(struct acl *acl) {
    uint16_t mask = 0;
    for (size_t i = 0; i < acl->count; i++) {
        if (acl->entries[i].tag == ACL_GROUP_OBJ || acl->entries[i].tag == ACL_USER ||
            acl->entries[i].tag == ACL_GROUP) mask |= acl->entries[i].perm;
    }
    acl_put(acl, ACL_MASK, mask, ACL_UNDEFINED_ID);
}

static int validate_acl(const struct acl *acl) {
    int user_obj = 0, group_obj = 0, other = 0, mask = 0;
    for (size_t i = 0; i < acl->count; i++) {
        if (acl->entries[i].perm > 7) return -1;
        user_obj += acl->entries[i].tag == ACL_USER_OBJ;
        group_obj += acl->entries[i].tag == ACL_GROUP_OBJ;
        other += acl->entries[i].tag == ACL_OTHER;
        mask += acl->entries[i].tag == ACL_MASK;
    }
    return user_obj == 1 && group_obj == 1 && other == 1 && mask <= 1 &&
           (!has_extended_entries(acl) || mask == 1) ? 0 : -1;
}

static int apply_spec(struct acl *access, struct acl *defaults, const char *spec,
                      int default_mode, int remove_entries, int *explicit_mask) {
    char *copy = strdup(spec);
    if (!copy) return -1;
    int status = 0;
    char *cursor = copy;
    while (cursor && *cursor) {
        char *next = strchr(cursor, ',');
        if (next) *next++ = 0;
        struct acl_entry entry = {0};
        int is_default = 0;
        if (parse_entry(cursor, default_mode, &entry, &is_default, !remove_entries) != 0) {
            status = -1;
            break;
        }
        struct acl *target = is_default ? defaults : access;
        if (entry.tag == ACL_MASK) explicit_mask[is_default] = 1;
        status = remove_entries ? acl_remove(target, entry.tag, entry.id)
                                : acl_put(target, entry.tag, entry.perm, entry.id);
        if (status != 0) break;
        cursor = next;
    }
    free(copy);
    return status;
}

static const char *principal_name(uint16_t tag, uint32_t id, int numeric,
                                  char buffer[32]) {
    if (!numeric) {
        if (tag == ACL_USER) {
            struct passwd *record = getpwuid(id);
            if (record) return record->pw_name;
        } else if (tag == ACL_GROUP) {
            struct group *record = getgrgid(id);
            if (record) return record->gr_name;
        }
    }
    snprintf(buffer, 32, "%u", id);
    return buffer;
}

static uint16_t acl_mask(const struct acl *acl) {
    for (size_t i = 0; i < acl->count; i++)
        if (acl->entries[i].tag == ACL_MASK) return acl->entries[i].perm;
    return 7;
}

static void print_acl(const struct acl *acl, int defaults, int numeric) {
    uint16_t mask = acl_mask(acl);
    for (size_t i = 0; i < acl->count; i++) {
        const struct acl_entry *entry = &acl->entries[i];
        char permissions[4], effective[4], id[32];
        format_permissions(entry->perm, permissions);
        const char *prefix = defaults ? "default:" : "";
        if (entry->tag == ACL_USER_OBJ) printf("%suser::%s", prefix, permissions);
        else if (entry->tag == ACL_USER)
            printf("%suser:%s:%s", prefix, principal_name(entry->tag, entry->id, numeric, id), permissions);
        else if (entry->tag == ACL_GROUP_OBJ) printf("%sgroup::%s", prefix, permissions);
        else if (entry->tag == ACL_GROUP)
            printf("%sgroup:%s:%s", prefix, principal_name(entry->tag, entry->id, numeric, id), permissions);
        else if (entry->tag == ACL_MASK) printf("%smask::%s", prefix, permissions);
        else if (entry->tag == ACL_OTHER) printf("%sother::%s", prefix, permissions);
        if (entry->tag == ACL_USER || entry->tag == ACL_GROUP || entry->tag == ACL_GROUP_OBJ) {
            uint16_t actual = entry->perm & mask;
            if (actual != entry->perm) {
                format_permissions(actual, effective);
                printf("\t#effective:%s", effective);
            }
        }
        putchar('\n');
    }
}

static int getfacl_main(int argc, char **argv) {
    int numeric = 0, absolute = 0, skip_base = 0, first_path = argc;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "-n") || !strcmp(argv[i], "--numeric")) numeric = 1;
        else if (!strcmp(argv[i], "--absolute-names")) absolute = 1;
        else if (!strcmp(argv[i], "-s") || !strcmp(argv[i], "--skip-base")) skip_base = 1;
        else if (argv[i][0] == '-') continue;
        else { first_path = i; break; }
    }
    if (first_path == argc) { fprintf(stderr, "getfacl: missing file operand\n"); return 1; }
    int status = 0;
    for (int i = first_path; i < argc; i++) {
        struct stat st;
        struct acl access = {0}, defaults = {0};
        if (stat(argv[i], &st) != 0 || read_acl(argv[i], 0, 1, &access) != 0) {
            fprintf(stderr, "getfacl: %s: %s\n", argv[i], strerror(errno));
            acl_free(&access);
            status = 1;
            continue;
        }
        int extended = has_extended_entries(&access) || acl_find(&access, ACL_MASK, ACL_UNDEFINED_ID);
        int have_defaults = read_acl(argv[i], 1, 0, &defaults) == 0;
        if (skip_base && !extended && !have_defaults) { acl_free(&access); acl_free(&defaults); continue; }
        const char *display = argv[i];
        if (!absolute) while (*display == '/') display++;
        char owner[32], group[32];
        printf("# file: %s\n", display);
        printf("# owner: %s\n", principal_name(ACL_USER, st.st_uid, numeric, owner));
        printf("# group: %s\n", principal_name(ACL_GROUP, st.st_gid, numeric, group));
        acl_sort(&access);
        print_acl(&access, 0, numeric);
        if (have_defaults) { acl_sort(&defaults); print_acl(&defaults, 1, numeric); }
        putchar('\n');
        acl_free(&access);
        acl_free(&defaults);
    }
    return status;
}

static int setfacl_main(int argc, char **argv) {
    const char *modify = NULL, *remove_spec = NULL, *set_spec = NULL;
    int default_mode = 0, no_mask = 0, remove_all = 0, remove_default = 0;
    int first_path = argc;
    for (int i = 1; i < argc; i++) {
        const char *arg = argv[i];
        if ((!strcmp(arg, "-m") || !strcmp(arg, "--modify")) && i + 1 < argc) modify = argv[++i];
        else if (!strncmp(arg, "--modify=", 9)) modify = arg + 9;
        else if ((!strcmp(arg, "-x") || !strcmp(arg, "--remove")) && i + 1 < argc) remove_spec = argv[++i];
        else if (!strncmp(arg, "--remove=", 9)) remove_spec = arg + 9;
        else if (!strcmp(arg, "--set") && i + 1 < argc) set_spec = argv[++i];
        else if (!strncmp(arg, "--set=", 6)) set_spec = arg + 6;
        else if (!strcmp(arg, "-d") || !strcmp(arg, "--default")) default_mode = 1;
        else if (!strcmp(arg, "-n") || !strcmp(arg, "--no-mask")) no_mask = 1;
        else if (!strcmp(arg, "-b") || !strcmp(arg, "--remove-all")) remove_all = 1;
        else if (!strcmp(arg, "-k") || !strcmp(arg, "--remove-default")) remove_default = 1;
        else if (!strcmp(arg, "-P") || !strcmp(arg, "--physical")) {}
        else if (arg[0] == '-') continue;
        else { first_path = i; break; }
    }
    if (first_path == argc) { fprintf(stderr, "setfacl: missing file operand\n"); return 2; }
    int status = 0;
    for (int i = first_path; i < argc; i++) {
        const char *path = argv[i];
        if (remove_all) {
            if (removexattr(path, "system.posix_acl_access") != 0 && errno != ENODATA) goto failure;
            if (removexattr(path, "system.posix_acl_default") != 0 && errno != ENODATA) goto failure;
            continue;
        }
        if (remove_default) {
            if (removexattr(path, "system.posix_acl_default") != 0 && errno != ENODATA) goto failure;
            continue;
        }
        struct acl access = {0}, defaults = {0};
        if (!set_spec && read_acl(path, 0, 1, &access) != 0) goto acl_failure;
        if (!set_spec && read_acl(path, 1, 1, &defaults) != 0) goto acl_failure;
        if (set_spec && default_mode) {
            struct stat st;
            if (stat(path, &st) != 0 || acl_from_mode(&access, st.st_mode) != 0) goto acl_failure;
        }
        int explicit_mask[2] = {0, 0};
        const char *spec = set_spec ? set_spec : modify ? modify : remove_spec;
        if (!spec || apply_spec(&access, &defaults, spec, default_mode,
                                remove_spec != NULL, explicit_mask) != 0) {
            errno = EINVAL;
            goto acl_failure;
        }
        struct acl *target = default_mode ? &defaults : &access;
        if (!no_mask && !explicit_mask[default_mode] && has_extended_entries(target))
            calculate_mask(target);
        if (validate_acl(target) != 0) { errno = EINVAL; goto acl_failure; }
        if (write_acl(path, default_mode, target) != 0) goto acl_failure;
        acl_free(&access);
        acl_free(&defaults);
        continue;
acl_failure:
        acl_free(&access);
        acl_free(&defaults);
failure:
        fprintf(stderr, "setfacl: %s: %s\n", path, strerror(errno));
        status = 1;
    }
    return status;
}

static void print_chacl_entries(const struct acl *acl) {
    for (size_t i = 0; i < acl->count; i++) {
        const struct acl_entry *entry = &acl->entries[i];
        char permissions[4];
        format_permissions(entry->perm, permissions);
        if (i) putchar(',');
        if (entry->tag == ACL_USER_OBJ) printf("u::%s", permissions);
        else if (entry->tag == ACL_USER) printf("u:%u:%s", entry->id, permissions);
        else if (entry->tag == ACL_GROUP_OBJ) printf("g::%s", permissions);
        else if (entry->tag == ACL_GROUP) printf("g:%u:%s", entry->id, permissions);
        else if (entry->tag == ACL_MASK) printf("m::%s", permissions);
        else if (entry->tag == ACL_OTHER) printf("o::%s", permissions);
    }
}

static int chacl_list(const char *path) {
    struct acl access = {0}, defaults = {0};
    if (read_acl(path, 0, 1, &access) != 0) {
        fprintf(stderr, "chacl: %s: %s\n", path, strerror(errno));
        return 1;
    }
    int have_defaults = read_acl(path, 1, 0, &defaults) == 0;
    acl_sort(&access);
    printf("%s [", path);
    print_chacl_entries(&access);
    if (have_defaults) {
        acl_sort(&defaults);
        putchar('/');
        print_chacl_entries(&defaults);
    }
    puts("]");
    acl_free(&access);
    acl_free(&defaults);
    return 0;
}

static size_t first_extended_entry(const struct acl *acl) {
    for (size_t i = 0; i < acl->count; i++) {
        if (acl->entries[i].tag == ACL_USER || acl->entries[i].tag == ACL_GROUP) return i;
    }
    return 0;
}

static int chacl_set_one(const char *path, const char *spec, int defaults) {
    struct acl access = {0}, default_acl = {0};
    int explicit_mask[2] = {0, 0};
    errno = EINVAL;
    if (apply_spec(&access, &default_acl, spec, defaults, 0, explicit_mask) != 0) {
        fprintf(stderr, "chacl: %s - Invalid argument\n", spec);
        acl_free(&access);
        acl_free(&default_acl);
        return 1;
    }
    struct acl *target = defaults ? &default_acl : &access;
    if (has_extended_entries(target) && !acl_find(target, ACL_MASK, ACL_UNDEFINED_ID)) {
        fprintf(stderr,
                "chacl: %s ACL '%s': Missing or wrong entry at entry %zu\n",
                defaults ? "default" : "access", spec, first_extended_entry(target));
        acl_free(&access);
        acl_free(&default_acl);
        return 1;
    }
    if (validate_acl(target) != 0) {
        fprintf(stderr, "chacl: %s - Invalid argument\n", spec);
        acl_free(&access);
        acl_free(&default_acl);
        return 1;
    }
    if (write_acl(path, defaults, target) != 0) {
        fprintf(stderr, "chacl: cannot set %s acl on \"%s\": %s\n",
                defaults ? "default" : "access", path, strerror(errno));
        acl_free(&access);
        acl_free(&default_acl);
        return 1;
    }
    acl_free(&access);
    acl_free(&default_acl);
    return 0;
}

static int chacl_remove_one(const char *path, int access, int defaults) {
    int status = 0;
    if (access && removexattr(path, "system.posix_acl_access") != 0 && errno != ENODATA) {
        fprintf(stderr, "chacl: %s: %s\n", path, strerror(errno));
        status = 1;
    }
    if (defaults && removexattr(path, "system.posix_acl_default") != 0 && errno != ENODATA) {
        fprintf(stderr, "chacl: %s: %s\n", path, strerror(errno));
        status = 1;
    }
    return status;
}

static int chacl_set_recursive(const char *path, const char *spec) {
    struct stat st;
    if (lstat(path, &st) != 0) {
        fprintf(stderr, "chacl: %s: %s\n", path, strerror(errno));
        return 1;
    }
    int status = 0;
    if (S_ISDIR(st.st_mode)) {
        DIR *dir = opendir(path);
        if (!dir) {
            fprintf(stderr, "chacl: %s: %s\n", path, strerror(errno));
            return 1;
        }
        struct dirent *entry;
        while ((entry = readdir(dir)) != NULL) {
            if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, "..")) continue;
            size_t path_len = strlen(path);
            size_t name_len = strlen(entry->d_name);
            if (path_len + name_len + 2 > PATH_MAX) {
                errno = ENAMETOOLONG;
                fprintf(stderr, "chacl: %s/%s: %s\n", path, entry->d_name, strerror(errno));
                status = 1;
                continue;
            }
            char child[PATH_MAX];
            snprintf(child, sizeof(child), "%s/%s", path, entry->d_name);
            status |= chacl_set_recursive(child, spec);
        }
        if (closedir(dir) != 0) status = 1;
    }
    return status | chacl_set_one(path, spec, 0);
}

static int chacl_main(int argc, char **argv) {
    if (argc >= 3 && !strcmp(argv[1], "-l")) {
        int status = 0;
        for (int i = 2; i < argc; i++) status |= chacl_list(argv[i]);
        return status;
    }

    if (argc >= 3 && (!strcmp(argv[1], "-R") || !strcmp(argv[1], "-D") ||
                      !strcmp(argv[1], "-B"))) {
        int remove_access = strcmp(argv[1], "-D") != 0;
        int remove_defaults = strcmp(argv[1], "-R") != 0;
        int status = 0;
        for (int i = 2; i < argc; i++)
            status |= chacl_remove_one(argv[i], remove_access, remove_defaults);
        return status;
    }

    if (argc >= 4 && !strcmp(argv[1], "-r")) {
        int status = 0;
        for (int i = 3; i < argc; i++) status |= chacl_set_recursive(argv[i], argv[2]);
        return status;
    }

    if (argc >= 5 && !strcmp(argv[1], "-b")) {
        int status = 0;
        for (int i = 4; i < argc; i++) {
            status |= chacl_set_one(argv[i], argv[2], 0);
            status |= chacl_set_one(argv[i], argv[3], 1);
        }
        return status;
    }

    int defaults = argc >= 4 && !strcmp(argv[1], "-d");
    if (argc < (defaults ? 4 : 3)) {
        fprintf(stderr, "chacl: missing ACL or file operand\n");
        return 1;
    }
    const char *spec = defaults ? argv[2] : argv[1];
    int first_path = defaults ? 3 : 2;
    int status = 0;
    for (int i = first_path; i < argc; i++) status |= chacl_set_one(argv[i], spec, defaults);
    return status;
}

int main(int argc, char **argv) {
    const char *program = base_name(argv[0]);
    if (!strcmp(program, "getfacl")) return getfacl_main(argc, argv);
    if (!strcmp(program, "setfacl")) return setfacl_main(argc, argv);
    return chacl_main(argc, argv);
}
