/* sleep_test.c — sleep N ms using nanosleep(), verify elapsed time */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

int main(int argc, char *argv[]) {
    int ms = 100; /* default 100ms */
    if (argc > 1) ms = atoi(argv[1]);

    struct timespec req;
    req.tv_sec = ms / 1000;
    req.tv_nsec = (long)(ms % 1000) * 1000000L;

    struct timespec before, after;
    clock_gettime(CLOCK_MONOTONIC, &before);

    nanosleep(&req, NULL);

    clock_gettime(CLOCK_MONOTONIC, &after);

    long elapsed_ms = (after.tv_sec - before.tv_sec) * 1000 +
                      (after.tv_nsec - before.tv_nsec) / 1000000;

    printf("requested=%dms\n", ms);
    printf("elapsed=%ldms\n", elapsed_ms);
    /* Allow 80% tolerance for timing jitter */
    printf("ok=%s\n", (elapsed_ms >= ms * 80 / 100) ? "yes" : "no");
    return 0;
}
