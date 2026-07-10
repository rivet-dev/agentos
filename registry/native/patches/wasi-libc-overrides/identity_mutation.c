#include <errno.h>
#include <unistd.h>

/*
 * POSIX getgroups(3p) and Linux getgroups(2):
 * https://pubs.opengroup.org/onlinepubs/9799919799/functions/getgroups.html
 * https://man7.org/linux/man-pages/man2/getgroups.2.html
 *
 * AgentOS exposes the guest's fixed kernel identity. Identity mutation is not
 * a guest capability, so same-identity changes are harmless no-ops and any
 * attempted privilege/identity transition fails with EPERM.
 */
int getgroups(int size, gid_t list[]) {
    if (size == 0)
        return 1;
    if (size < 1 || list == NULL) {
        errno = EINVAL;
        return -1;
    }
    list[0] = getgid();
    return 1;
}

static int require_uid(uid_t requested) {
    if (requested == getuid() || requested == geteuid())
        return 0;
    errno = EPERM;
    return -1;
}

static int require_gid(gid_t requested) {
    if (requested == getgid() || requested == getegid())
        return 0;
    errno = EPERM;
    return -1;
}

int setuid(uid_t uid) { return require_uid(uid); }
int seteuid(uid_t uid) { return require_uid(uid); }
int setgid(gid_t gid) { return require_gid(gid); }
int setegid(gid_t gid) { return require_gid(gid); }
