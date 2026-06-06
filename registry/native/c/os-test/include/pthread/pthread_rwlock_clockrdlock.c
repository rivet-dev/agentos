#include <pthread.h>
#ifdef pthread_rwlock_clockrdlock
#undef pthread_rwlock_clockrdlock
#endif
int (*foo)(pthread_rwlock_t *restrict, clockid_t, const struct timespec *restrict) = pthread_rwlock_clockrdlock;
int main(void) { return 0; }
