#include <pthread.h>
#ifdef pthread_mutex_clocklock
#undef pthread_mutex_clocklock
#endif
int (*foo)(pthread_mutex_t *restrict, clockid_t, const struct timespec *restrict) = pthread_mutex_clocklock;
int main(void) { return 0; }
