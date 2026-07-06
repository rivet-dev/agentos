/* delayed_kill.c -- sleep briefly, then send a signal to the target pid */
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argc, char *argv[]) {
    if (argc < 4) {
        fprintf(stderr, "usage: delayed_kill <delay_ms> <pid> <signal>\n");
        return 1;
    }

    int delay_ms = atoi(argv[1]);
    pid_t pid = (pid_t)atoi(argv[2]);
    int signal_number = atoi(argv[3]);

    if (delay_ms > 0) {
        usleep((useconds_t)delay_ms * 1000u);
    }

    return kill(pid, signal_number) == 0 ? 0 : 1;
}
