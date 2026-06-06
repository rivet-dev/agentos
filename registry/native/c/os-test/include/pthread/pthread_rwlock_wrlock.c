#include <pthread.h>
#ifdef pthread_rwlock_wrlock
#undef pthread_rwlock_wrlock
#endif
int (*foo)(pthread_rwlock_t *) = pthread_rwlock_wrlock;
int main(void) { return 0; }
