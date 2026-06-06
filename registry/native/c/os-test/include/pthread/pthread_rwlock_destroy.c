#include <pthread.h>
#ifdef pthread_rwlock_destroy
#undef pthread_rwlock_destroy
#endif
int (*foo)(pthread_rwlock_t *) = pthread_rwlock_destroy;
int main(void) { return 0; }
