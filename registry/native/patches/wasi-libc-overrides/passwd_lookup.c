#include <errno.h>
#include <pwd.h>
#include <string.h>
#include <unistd.h>

/* passwd database contract: https://man7.org/linux/man-pages/man5/passwd.5.html */
static int copy_passwd(const struct passwd *source, struct passwd *target,
                       char *buffer, size_t length, struct passwd **result) {
    const char *fields[] = {
        source->pw_name, source->pw_passwd, source->pw_gecos,
        source->pw_dir, source->pw_shell,
    };
    char **outputs[] = {
        &target->pw_name, &target->pw_passwd, &target->pw_gecos,
        &target->pw_dir, &target->pw_shell,
    };
    size_t required = 0;
    for (size_t index = 0; index < 5; ++index)
        required += strlen(fields[index] ? fields[index] : "") + 1;
    if (length < required) {
        *result = NULL;
        return ERANGE;
    }

    char *cursor = buffer;
    for (size_t index = 0; index < 5; ++index) {
        const char *field = fields[index] ? fields[index] : "";
        const size_t field_length = strlen(field) + 1;
        memcpy(cursor, field, field_length);
        *outputs[index] = cursor;
        cursor += field_length;
    }
    target->pw_uid = source->pw_uid;
    target->pw_gid = source->pw_gid;
    *result = target;
    return 0;
}

int getpwuid_r(uid_t uid, struct passwd *target, char *buffer, size_t length,
               struct passwd **result) {
    if (target == NULL || buffer == NULL || result == NULL)
        return EINVAL;
    struct passwd *source = getpwuid(uid);
    if (source == NULL) {
        *result = NULL;
        return 0;
    }
    return copy_passwd(source, target, buffer, length, result);
}

struct passwd *getpwnam(const char *name) {
    if (name == NULL)
        return NULL;
    struct passwd *current = getpwuid(getuid());
    return current != NULL && strcmp(current->pw_name, name) == 0 ? current : NULL;
}
