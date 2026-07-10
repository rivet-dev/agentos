#include <errno.h>
#include <signal.h>
#include <string.h>

/*
 * POSIX pthread_sigmask(3p). AgentOS signals are cooperatively delivered, so a
 * worker-local mask is the authoritative state consulted by the thread bridge.
 * https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_sigmask.html
 */
int pthread_sigmask(int how, const sigset_t *set, sigset_t *old_set) {
    static _Thread_local sigset_t current;
    unsigned char *current_bytes = (unsigned char *)(void *)&current;
    const unsigned char *set_bytes = (const unsigned char *)(const void *)set;

    if (old_set != NULL)
        memcpy(old_set, &current, sizeof(current));
    if (set == NULL)
        return 0;
    if (how != SIG_BLOCK && how != SIG_UNBLOCK && how != SIG_SETMASK)
        return EINVAL;
    if (how == SIG_SETMASK) {
        memcpy(&current, set, sizeof(current));
        return 0;
    }
    for (size_t index = 0; index < sizeof(current); index++) {
        if (how == SIG_BLOCK)
            current_bytes[index] |= set_bytes[index];
        else
            current_bytes[index] &= (unsigned char)~set_bytes[index];
    }
    return 0;
}
