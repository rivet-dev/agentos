/* isatty_test.c — detect if stdout/stderr/stdin are terminals */
#include <stdio.h>
#include <unistd.h>

int main(void) {
    printf("stdin isatty: %d\n", isatty(STDIN_FILENO));
    printf("stdout isatty: %d\n", isatty(STDOUT_FILENO));
    printf("stderr isatty: %d\n", isatty(STDERR_FILENO));
    return 0;
}
