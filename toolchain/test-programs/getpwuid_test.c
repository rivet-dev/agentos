/* getpwuid_test.c — validate getpwuid() returns correct passwd entry */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pwd.h>
#include <unistd.h>

int main(void) {
    int failures = 0;

    struct passwd *pw = getpwuid(1000);
    if (!pw) {
        printf("getpwuid: NULL\n");
        printf("pw_name_nonempty: no\n");
        printf("pw_uid_match: no\n");
        printf("pw_gid_valid: no\n");
        printf("pw_dir_nonempty: no\n");
        printf("pw_shell_nonempty: no\n");
        return 1;
    }

    printf("getpwuid: ok\n");

    /* pw_name is non-empty */
    int name_ok = pw->pw_name != NULL && strlen(pw->pw_name) > 0;
    printf("pw_name_nonempty: %s\n", name_ok ? "yes" : "no");
    if (!name_ok) failures++;

    /* pw_uid matches requested uid */
    int uid_ok = pw->pw_uid == 1000;
    printf("pw_uid_match: %s\n", uid_ok ? "yes" : "no");
    if (!uid_ok) failures++;

    /* pw_gid is valid (positive) */
    int gid_ok = pw->pw_gid > 0;
    printf("pw_gid_valid: %s\n", gid_ok ? "yes" : "no");
    if (!gid_ok) failures++;

    /* pw_dir is non-empty */
    int dir_ok = pw->pw_dir != NULL && strlen(pw->pw_dir) > 0;
    printf("pw_dir_nonempty: %s\n", dir_ok ? "yes" : "no");
    if (!dir_ok) failures++;

    /* pw_shell is non-empty */
    int shell_ok = pw->pw_shell != NULL && strlen(pw->pw_shell) > 0;
    printf("pw_shell_nonempty: %s\n", shell_ok ? "yes" : "no");
    if (!shell_ok) failures++;

    /* Print actual values for debugging */
    printf("pw_name=%s\n", pw->pw_name);
    printf("pw_uid=%u\n", (unsigned)pw->pw_uid);
    printf("pw_gid=%u\n", (unsigned)pw->pw_gid);
    printf("pw_dir=%s\n", pw->pw_dir);
    printf("pw_shell=%s\n", pw->pw_shell);

    return failures > 0 ? 1 : 0;
}
