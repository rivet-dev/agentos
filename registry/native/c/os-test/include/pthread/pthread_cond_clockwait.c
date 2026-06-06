#include <pthread.h>
#ifdef pthread_cond_clockwait
#undef pthread_cond_clockwait
#endif
int (*foo)(pthread_cond_t *restrict, pthread_mutex_t *restrict, clockid_t, const struct timespec *restrict) = pthread_cond_clockwait;
int main(void) { return 0; }
