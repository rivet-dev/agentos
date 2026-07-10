#include <errno.h>
#include <grp.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>

/* Bounded group database view; see grp(5) and getgrnam(3). */
static int fill_group(gid_t gid, struct group *group, char *buffer,
                      size_t length, struct group **result) {
    static const char name[] = "agentos";
    static const char passwd[] = "x";
    const size_t strings = sizeof(name) + sizeof(passwd);
    const size_t aligned = (strings + sizeof(char *) - 1) & ~(sizeof(char *) - 1);

    if (gid != getgid()) {
        *result = NULL;
        return 0;
    }
    if (length < aligned + sizeof(char *))
        return ERANGE;
    memcpy(buffer, name, sizeof(name));
    memcpy(buffer + sizeof(name), passwd, sizeof(passwd));
    group->gr_name = buffer;
    group->gr_passwd = buffer + sizeof(name);
    group->gr_gid = gid;
    group->gr_mem = (char **)(void *)(buffer + aligned);
    group->gr_mem[0] = NULL;
    *result = group;
    return 0;
}

int getgrgid_r(gid_t gid, struct group *group, char *buffer, size_t length,
               struct group **result) {
    if (group == NULL || buffer == NULL || result == NULL)
        return EINVAL;
    return fill_group(gid, group, buffer, length, result);
}

int getgrnam_r(const char *name, struct group *group, char *buffer,
               size_t length, struct group **result) {
    if (name == NULL || strcmp(name, "agentos") != 0) {
        if (result != NULL)
            *result = NULL;
        return name == NULL || result == NULL ? EINVAL : 0;
    }
    return getgrgid_r(getgid(), group, buffer, length, result);
}

struct group *getgrgid(gid_t gid) {
    static _Thread_local struct group group;
    static _Thread_local char buffer[64];
    struct group *result;
    return getgrgid_r(gid, &group, buffer, sizeof(buffer), &result) == 0 ? result : NULL;
}

struct group *getgrnam(const char *name) {
    static _Thread_local struct group group;
    static _Thread_local char buffer[64];
    struct group *result;
    return getgrnam_r(name, &group, buffer, sizeof(buffer), &result) == 0 ? result : NULL;
}

/* Mutating trusted identity is outside the guest capability set. */
int setgroups(size_t count, const gid_t *groups) {
    (void)count;
    (void)groups;
    errno = EPERM;
    return -1;
}

int initgroups(const char *user, gid_t group) {
    (void)user;
    (void)group;
    errno = EPERM;
    return -1;
}
