/* sigaction_behavior.c -- verify sigaction query, SA_RESETHAND, and SA_RESTART */
#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

static volatile sig_atomic_t reset_handler_calls = 0;
static volatile sig_atomic_t restart_handler_calls = 0;

static void reset_handler(int sig) {
    (void)sig;
    reset_handler_calls++;
}

static void restart_handler(int sig) {
    (void)sig;
    restart_handler_calls++;
}

static int install_action(int sig, void (*handler)(int), int flags, int masked_sig) {
    struct sigaction action;
    memset(&action, 0, sizeof(action));
    sigemptyset(&action.sa_mask);
    if (masked_sig > 0) {
        sigaddset(&action.sa_mask, masked_sig);
    }
    action.sa_flags = flags;
    action.sa_handler = handler;
    action.sa_restorer = NULL;
    return sigaction(sig, &action, NULL);
}

int main(void) {
    struct sigaction current;

    /* Query round-trip: handler, mask, and flags survive the libc-facing API. */
    if (install_action(SIGINT, restart_handler, SA_RESTART | SA_RESETHAND, SIGTERM) != 0) {
        perror("sigaction query install");
        return 1;
    }
    memset(&current, 0, sizeof(current));
    if (sigaction(SIGINT, NULL, &current) != 0) {
        perror("sigaction query read");
        return 1;
    }
    printf("sigaction_query_mask_sigterm=%s\n", sigismember(&current.sa_mask, SIGTERM) == 1 ? "yes" : "no");
    printf("sigaction_query_flags=%s\n",
        (current.sa_flags & (SA_RESTART | SA_RESETHAND)) == (SA_RESTART | SA_RESETHAND) ? "yes" : "no");

    /* SA_RESETHAND: first delivery runs the handler and resets to SIG_DFL. */
    if (install_action(SIGUSR1, reset_handler, SA_RESETHAND, 0) != 0) {
        perror("sigaction SA_RESETHAND install");
        return 1;
    }
    if (kill(getpid(), SIGUSR1) != 0) {
        perror("kill SIGUSR1");
        return 1;
    }
    memset(&current, 0, sizeof(current));
    if (sigaction(SIGUSR1, NULL, &current) != 0) {
        perror("sigaction SA_RESETHAND read");
        return 1;
    }
    printf("sa_resethand_handler_calls=%d\n", (int)reset_handler_calls);
    printf("sa_resethand_reset=%s\n", current.sa_handler == SIG_DFL ? "yes" : "no");

    /* SA_RESTART: accept() should resume after SIGALRM and still take the child connection. */
    int listener_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (listener_fd < 0) {
        perror("socket");
        return 1;
    }

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    int port = 30000 + (getpid() % 10000);
    addr.sin_port = htons((uint16_t)port);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);

    if (bind(listener_fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        perror("bind");
        close(listener_fd);
        return 1;
    }
    if (listen(listener_fd, 1) != 0) {
        perror("listen");
        close(listener_fd);
        return 1;
    }

    char delay_arg[16];
    char port_arg[16];
    snprintf(delay_arg, sizeof(delay_arg), "%d", 1500);
    snprintf(port_arg, sizeof(port_arg), "%d", port);

    char *echo_argv[] = {"delayed_tcp_echo", delay_arg, port_arg, NULL};
    pid_t child;
    int spawn_err = posix_spawnp(&child, "delayed_tcp_echo", NULL, NULL, echo_argv, environ);
    if (spawn_err != 0) {
        fprintf(stderr, "posix_spawn delayed_tcp_echo failed: %d\n", spawn_err);
        close(listener_fd);
        return 1;
    }

    if (install_action(SIGALRM, restart_handler, SA_RESTART, 0) != 0) {
        perror("sigaction SA_RESTART install");
        close(listener_fd);
        return 1;
    }

    char signal_delay_arg[16];
    char self_pid_arg[16];
    char signal_arg[16];
    snprintf(signal_delay_arg, sizeof(signal_delay_arg), "%d", 1000);
    snprintf(self_pid_arg, sizeof(self_pid_arg), "%d", (int)getpid());
    snprintf(signal_arg, sizeof(signal_arg), "%d", SIGALRM);

    char *signal_argv[] = {"delayed_kill", signal_delay_arg, self_pid_arg, signal_arg, NULL};
    pid_t signaler;
    spawn_err = posix_spawnp(&signaler, "delayed_kill", NULL, NULL, signal_argv, environ);
    if (spawn_err != 0) {
        fprintf(stderr, "posix_spawn delayed_kill failed: %d\n", spawn_err);
        close(listener_fd);
        return 1;
    }

    int client_fd = accept(listener_fd, NULL, NULL);
    if (client_fd < 0) {
        perror("accept");
        close(listener_fd);
        return 1;
    }

    char buf[16] = {0};
    ssize_t n = recv(client_fd, buf, sizeof(buf) - 1, 0);
    if (n < 0) {
        perror("recv");
        close(client_fd);
        close(listener_fd);
        return 1;
    }
    buf[n] = '\0';

    if (send(client_fd, "pong", 4, 0) != 4) {
        perror("send");
        close(client_fd);
        close(listener_fd);
        return 1;
    }

    int status = 0;
    if (waitpid(child, &status, 0) < 0) {
        perror("waitpid");
        close(client_fd);
        close(listener_fd);
        return 1;
    }
    int signal_status = 0;
    if (waitpid(signaler, &signal_status, 0) < 0) {
        perror("waitpid signaler");
        close(client_fd);
        close(listener_fd);
        return 1;
    }

    close(client_fd);
    close(listener_fd);

    printf("sa_restart_handler_calls=%d\n", (int)restart_handler_calls);
    printf("sa_restart_accept=%s\n", strcmp(buf, "hello") == 0 ? "yes" : "no");
    printf("sa_restart_child_exit=%d\n",
        WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status));
    printf("sa_restart_signal_exit=%d\n",
        WIFEXITED(signal_status) ? WEXITSTATUS(signal_status) : 128 + WTERMSIG(signal_status));

    if (strcmp(buf, "hello") != 0) {
        return 1;
    }
    if (reset_handler_calls != 1 || current.sa_handler != SIG_DFL) {
        return 1;
    }
    if (restart_handler_calls < 1) {
        return 1;
    }
    if (!WIFEXITED(signal_status) || WEXITSTATUS(signal_status) != 0) {
        return 1;
    }
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 1;
}
