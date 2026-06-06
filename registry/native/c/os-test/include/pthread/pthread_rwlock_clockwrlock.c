#include <pthread.h>
#ifdef pthread_rwlock_clockwrlock
#undef pthread_rwlock_clockwrlock
#endif
int (*foo)(pthread_rwlock_t *restrict, clockid_t, const struct timespec *restrict) = pthread_rwlock_clockwrlock;
int main(void) { return 0; }
