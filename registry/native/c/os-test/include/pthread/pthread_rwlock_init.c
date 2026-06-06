#include <pthread.h>
#ifdef pthread_rwlock_init
#undef pthread_rwlock_init
#endif
int (*foo)(pthread_rwlock_t *restrict, const pthread_rwlockattr_t *restrict) = pthread_rwlock_init;
int main(void) { return 0; }
