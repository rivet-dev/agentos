#include <pthread.h>
#ifdef pthread_barrier_init
#undef pthread_barrier_init
#endif
int (*foo)(pthread_barrier_t *restrict, const pthread_barrierattr_t *restrict, unsigned) = pthread_barrier_init;
int main(void) { return 0; }
