#include <pthread.h>
#ifdef pthread_rwlock_timedwrlock
#undef pthread_rwlock_timedwrlock
#endif
int (*foo)(pthread_rwlock_t *restrict, const struct timespec *restrict) = pthread_rwlock_timedwrlock;
int main(void) { return 0; }
