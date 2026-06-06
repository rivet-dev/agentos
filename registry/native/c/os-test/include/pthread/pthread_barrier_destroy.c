#include <pthread.h>
#ifdef pthread_barrier_destroy
#undef pthread_barrier_destroy
#endif
int (*foo)(pthread_barrier_t *) = pthread_barrier_destroy;
int main(void) { return 0; }
