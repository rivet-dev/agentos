#include <pthread.h>
#ifdef pthread_mutex_timedlock
#undef pthread_mutex_timedlock
#endif
int (*foo)(pthread_mutex_t *restrict, const struct timespec *restrict) = pthread_mutex_timedlock;
int main(void) { return 0; }
