#include <pthread.h>
#ifdef pthread_rwlock_trywrlock
#undef pthread_rwlock_trywrlock
#endif
int (*foo)(pthread_rwlock_t *) = pthread_rwlock_trywrlock;
int main(void) { return 0; }
