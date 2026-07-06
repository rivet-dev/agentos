/* userinfo.c — print uid, gid, euid, egid */
#include <stdio.h>
#include <sys/types.h>
#include <unistd.h>

int main(void) {
    printf("uid=%u\n", (unsigned)getuid());
    printf("gid=%u\n", (unsigned)getgid());
    printf("euid=%u\n", (unsigned)geteuid());
    printf("egid=%u\n", (unsigned)getegid());
    return 0;
}
