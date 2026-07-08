/* signal_handler.c — cooperative sigaction handling test for WasmVM.
 *
 * Registers a SIGINT handler via sigaction() with sa_mask + SA_RESTART +
 * SA_RESETHAND, then busy-loops with sleep syscalls (each sleep is a syscall
 * boundary where pending signals are delivered). The test runner inspects the
 * kernel registration and verifies the handler fires.
 *
 * Usage: signal_handler
 * Output:
 *   handler_registered
 *   waiting
 *   caught_signal=2
 */
#include <signal.h>
#include <stdio.h>
#include <unistd.h>

static volatile int got_signal = 0;

static void handler(int sig) {
    got_signal = sig;
}

int main(void) {
    struct sigaction action;
    sigemptyset(&action.sa_mask);
    sigaddset(&action.sa_mask, SIGTERM);
    action.sa_flags = SA_RESTART | SA_RESETHAND;
    action.sa_handler = handler;
    action.sa_restorer = NULL;

    if (sigaction(SIGINT, &action, NULL) != 0) {
        perror("sigaction");
        return 1;
    }

    printf("handler_registered\n");
    fflush(stdout);

    printf("waiting\n");
    fflush(stdout);

    /* Busy-loop with sleep — each usleep is a syscall boundary where
     * the JS worker checks for pending signals and invokes the trampoline. */
    for (int i = 0; i < 1000 && !got_signal; i++) {
        usleep(10000);  /* 10ms */
    }

    if (got_signal) {
        printf("caught_signal=%d\n", got_signal);
    } else {
        printf("timeout_no_signal\n");
    }

    return got_signal ? 0 : 1;
}
