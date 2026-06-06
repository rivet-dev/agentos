#include <pthread.h>
#ifdef pthread_mutex_init
#undef pthread_mutex_init
#endif
int (*foo)(pthread_mutex_t *restrict, const pthread_mutexattr_t *restrict) = pthread_mutex_init;
int main(void) { return 0; }
