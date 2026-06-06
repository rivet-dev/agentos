#include <pthread.h>
#ifdef pthread_rwlockattr_destroy
#undef pthread_rwlockattr_destroy
#endif
int (*foo)(pthread_rwlockattr_t *) = pthread_rwlockattr_destroy;
int main(void) { return 0; }
