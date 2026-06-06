#include <pthread.h>
#ifdef pthread_barrier_wait
#undef pthread_barrier_wait
#endif
int (*foo)(pthread_barrier_t *) = pthread_barrier_wait;
int main(void) { return 0; }
