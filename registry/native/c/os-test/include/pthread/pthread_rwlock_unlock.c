#include <pthread.h>
#ifdef pthread_rwlock_unlock
#undef pthread_rwlock_unlock
#endif
int (*foo)(pthread_rwlock_t *) = pthread_rwlock_unlock;
int main(void) { return 0; }
