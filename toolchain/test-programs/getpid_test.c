/* getpid_test.c — print PID (must NOT be hardcoded 42 in WASM) */
#include <stdio.h>
#include <unistd.h>

int main(void) {
    pid_t pid1 = getpid();
    pid_t pid2 = getpid();
    printf("pid=%d\n", pid1);
    /* Vanilla wasi-libc returns hardcoded 42; patched returns real PID */
    printf("pid_positive=%s\n", pid1 > 0 ? "yes" : "no");
    printf("pid_not_42=%s\n", pid1 != 42 ? "yes" : "no");
    /* Two calls must return the same PID */
    printf("pid_consistent=%s\n", pid1 == pid2 ? "yes" : "no");
    return 0;
}
