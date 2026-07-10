#include <signal.h>
#include <unistd.h>

/* POSIX raise(3p): https://pubs.opengroup.org/onlinepubs/9799919799/functions/raise.html */
int raise(int signal_number) {
    return kill(getpid(), signal_number);
}

/* wasi-libc uses callable sentinels because table index 1 is a valid pointer. */
void __SIG_IGN(int signal_number) {
    (void)signal_number;
}

void __SIG_ERR(int signal_number) {
    (void)signal_number;
}
