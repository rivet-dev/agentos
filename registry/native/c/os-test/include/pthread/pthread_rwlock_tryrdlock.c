#include <pthread.h>
#ifdef pthread_rwlock_tryrdlock
#undef pthread_rwlock_tryrdlock
#endif
int (*foo)(pthread_rwlock_t *) = pthread_rwlock_tryrdlock;
int main(void) { return 0; }
