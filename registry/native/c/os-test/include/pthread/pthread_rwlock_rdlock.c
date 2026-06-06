#include <pthread.h>
#ifdef pthread_rwlock_rdlock
#undef pthread_rwlock_rdlock
#endif
int (*foo)(pthread_rwlock_t *) = pthread_rwlock_rdlock;
int main(void) { return 0; }
