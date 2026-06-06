#include <pthread.h>
#ifdef pthread_rwlock_timedrdlock
#undef pthread_rwlock_timedrdlock
#endif
int (*foo)(pthread_rwlock_t *restrict, const struct timespec *restrict) = pthread_rwlock_timedrdlock;
int main(void) { return 0; }
