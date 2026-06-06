#include <pthread.h>
#ifdef pthread_condattr_getclock
#undef pthread_condattr_getclock
#endif
int (*foo)(const pthread_condattr_t *restrict, clockid_t *restrict) = pthread_condattr_getclock;
int main(void) { return 0; }
