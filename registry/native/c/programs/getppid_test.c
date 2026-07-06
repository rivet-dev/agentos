/* getppid_test.c — print parent PID (must not trap in WASM) */
#include <stdio.h>
#include <unistd.h>

int main(void) {
    pid_t ppid = getppid();
    printf("ppid=%d\n", ppid);
    printf("ppid_nonnegative=%s\n", ppid >= 0 ? "yes" : "no");
    return 0;
}
