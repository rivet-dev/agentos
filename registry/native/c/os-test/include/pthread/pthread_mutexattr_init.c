#include <pthread.h>
#ifdef pthread_mutexattr_init
#undef pthread_mutexattr_init
#endif
int (*foo)(pthread_mutexattr_t *) = pthread_mutexattr_init;
int main(void) { return 0; }
