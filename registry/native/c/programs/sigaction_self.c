#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static volatile sig_atomic_t handler_calls = 0;

static void handler(int sig) {
    (void)sig;
    handler_calls++;
}

int main(void) {
    struct sigaction action;
    struct sigaction current;
    memset(&action, 0, sizeof(action));
    sigemptyset(&action.sa_mask);
    action.sa_flags = SA_RESETHAND;
    action.sa_handler = handler;

    if (sigaction(SIGUSR1, &action, NULL) != 0) {
        perror("sigaction install");
        return 1;
    }
    if (kill(getpid(), SIGUSR1) != 0) {
        perror("kill self");
        return 1;
    }
    memset(&current, 0, sizeof(current));
    if (sigaction(SIGUSR1, NULL, &current) != 0) {
        perror("sigaction query");
        return 1;
    }

    printf("self_signal_handler_calls=%d\n", (int)handler_calls);
    printf("self_signal_reset=%s\n", current.sa_handler == SIG_DFL ? "yes" : "no");

    return handler_calls == 1 && current.sa_handler == SIG_DFL ? 0 : 1;
}
