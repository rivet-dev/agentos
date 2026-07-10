#include <stdatomic.h>
#include <sys/stat.h>

/* POSIX umask(2): https://pubs.opengroup.org/onlinepubs/9799919799/functions/umask.html */
static _Atomic unsigned int current_umask = 0022;

mode_t umask(mode_t mask) {
    return (mode_t)atomic_exchange_explicit(
        &current_umask, (unsigned int)mask & 0777u, memory_order_relaxed);
}
