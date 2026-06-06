#include <pthread.h>
#ifdef pthread_cond_timedwait
#undef pthread_cond_timedwait
#endif
int (*foo)(pthread_cond_t *restrict, pthread_mutex_t *restrict, const struct timespec *restrict) = pthread_cond_timedwait;
int main(void) { return 0; }
