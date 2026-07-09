#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <pwd.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int require(int condition, const char *label) {
    printf("%s: %s\n", label, condition ? "ok" : "failed");
    return condition ? 0 : 1;
}

int main(void) {
    int failures = 0;
    uid_t uid, euid, suid;
    gid_t gid, egid, sgid;
    gid_t groups[] = {1000, 1001};

    struct passwd *pw = getpwnam("fsgqa");
    failures += require(pw && pw->pw_uid == 1000 && pw->pw_gid == 1000 &&
                            strcmp(pw->pw_dir, "/home/fsgqa") == 0,
                        "getpwnam-fsgqa");
    struct passwd pw_reentrant;
    struct passwd *pw_result = NULL;
    char pw_storage[256];
    failures += require(getpwuid_r(1001, &pw_reentrant, pw_storage,
                                   sizeof(pw_storage), &pw_result) == 0 &&
                            pw_result == &pw_reentrant &&
                            strcmp(pw_result->pw_name, "fsgqa2") == 0,
                        "getpwuid-r-fsgqa2");
    char tiny_pw_storage[4];
    pw_result = (struct passwd *)1;
    failures += require(getpwnam_r("fsgqa", &pw_reentrant, tiny_pw_storage,
                                   sizeof(tiny_pw_storage), &pw_result) == ERANGE &&
                            pw_result == NULL,
                        "getpwnam-r-erange");
    setpwent();
    int passwd_count = 0;
    while (getpwent() != NULL) passwd_count++;
    endpwent();
    failures += require(passwd_count == 6, "getpwent-all-accounts");

    struct group *gr = getgrgid(1000);
    failures += require(gr && strcmp(gr->gr_name, "fsgqa") == 0 &&
                            gr->gr_mem && gr->gr_mem[0] &&
                            strcmp(gr->gr_mem[0], "fsgqa") == 0,
                        "getgrgid-fsgqa");
    struct group gr_reentrant;
    struct group *gr_result = NULL;
    char gr_storage[256];
    failures += require(getgrnam_r("fsgqa2", &gr_reentrant, gr_storage,
                                   sizeof(gr_storage), &gr_result) == 0 &&
                            gr_result == &gr_reentrant && gr_result->gr_gid == 1001,
                        "getgrnam-r-fsgqa2");
    setgrent();
    int group_count = 0;
    while (getgrent() != NULL) group_count++;
    endgrent();
    failures += require(group_count == 6, "getgrent-all-groups");
    gid_t listed_groups[4] = {0};
    int listed_count = 4;
    failures += require(getgrouplist("fsgqa", 1000, listed_groups,
                                    &listed_count) == 1 &&
                            listed_count == 1 && listed_groups[0] == 1000,
                        "getgrouplist-fsgqa");

    failures += require(getresuid(&uid, &euid, &suid) == 0,
                        "getresuid-root");
    failures += require(uid == 0 && euid == 0 && suid == 0,
                        "root-uids");
    failures += require(getresgid(&gid, &egid, &sgid) == 0,
                        "getresgid-root");
    failures += require(gid == 0 && egid == 0 && sgid == 0,
                        "root-gids");

    int fd = open("/mnt/test/c-owned", O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (fd < 0) perror("open /mnt/test/c-owned");
    failures += require(fd >= 0, "create-owned-file");
    if (fd >= 0) {
        failures += require(write(fd, "c", 1) == 1, "write-owned-file");
        failures += require(fchown(fd, 1000, 1000) == 0, "fchown-owned-file");
        struct stat st;
        failures += require(fstat(fd, &st) == 0, "fstat-owned-file");
        failures += require(st.st_uid == 1000 && st.st_gid == 1000,
                            "fstat-owner-ids");
        close(fd);
    }
    fd = open("/mnt/test/c-root-only", O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (fd < 0) perror("open /mnt/test/c-root-only");
    failures += require(fd >= 0, "create-root-file");
    if (fd >= 0) {
        struct stat root_stat;
        failures += require(fstat(fd, &root_stat) == 0, "fstat-root-file");
        failures += require(root_stat.st_uid == 0 && root_stat.st_gid == 0,
                            "root-file-owner-before-drop");
        failures += require((root_stat.st_mode & 0777) == 0600,
                            "open-create-mode");
        failures += require(fchmod(fd, 0600) == 0, "fchmod-root-file");
        close(fd);
    }
    unlink("/mnt/test/c-owned-link");
    failures += require(symlink("/mnt/test/c-owned", "/mnt/test/c-owned-link") == 0,
                        "create-owned-symlink");
    errno = 0;
    int lchown_result = lchown("/mnt/test/c-owned-link", 1000, 1000);
    failures += require(lchown_result == 0, "lchown-owned-symlink");
    struct stat link_stat;
    failures += require(lstat("/mnt/test/c-owned-link", &link_stat) == 0,
                        "lstat-owned-symlink");
    failures += require(link_stat.st_uid == 1000 && link_stat.st_gid == 1000,
                        "lstat-owner-ids");

    failures += require(setgroups(2, groups) == 0,
                        "setgroups-root");
    failures += require(setgid(1000) == 0, "setgid-fsgqa");
    failures += require(setuid(1000) == 0, "setuid-fsgqa");
    failures += require(getuid() == 1000 && geteuid() == 1000,
                        "dropped-uids");
    failures += require(getgid() == 1000 && getegid() == 1000,
                        "dropped-gids");
    struct stat root_stat;
    failures += require(stat("/mnt/test/c-root-only", &root_stat) == 0,
                        "stat-root-file-after-drop");
    failures += require(root_stat.st_uid == 0 && root_stat.st_gid == 0,
                        "root-file-owner-after-drop");
    errno = 0;
    int access_result = access("/mnt/test/c-owned", R_OK | W_OK);
    failures += require(access_result == 0, "owner-access");
    errno = 0;
    access_result = access("/mnt/test/c-root-only", R_OK);
    failures += require(access_result == -1 && errno == EACCES, "root-file-denied");

    gid_t actual[4] = {0};
    int count = getgroups(4, actual);
    failures += require(count == 2 && actual[0] == 1000 && actual[1] == 1001,
                        "supplementary-groups");

    errno = 0;
    failures += require(setuid(0) == -1 && errno == EPERM,
                        "cannot-regain-root");
    return failures == 0 ? 0 : 1;
}
