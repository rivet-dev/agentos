#include <pthread.h>
#ifdef pthread_rwlockattr_init
#undef pthread_rwlockattr_init
#endif
int (*foo)(pthread_rwlockattr_t *) = pthread_rwlockattr_init;
int main(void) { return 0; }
