#include <pthread.h>
#ifdef pthread_mutexattr_destroy
#undef pthread_mutexattr_destroy
#endif
int (*foo)(pthread_mutexattr_t *) = pthread_mutexattr_destroy;
int main(void) { return 0; }
