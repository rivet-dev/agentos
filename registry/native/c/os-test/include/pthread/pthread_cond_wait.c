#include <pthread.h>
#ifdef pthread_cond_wait
#undef pthread_cond_wait
#endif
int (*foo)(pthread_cond_t *restrict, pthread_mutex_t *restrict) = pthread_cond_wait;
int main(void) { return 0; }
