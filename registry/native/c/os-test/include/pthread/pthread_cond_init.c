#include <pthread.h>
#ifdef pthread_cond_init
#undef pthread_cond_init
#endif
int (*foo)(pthread_cond_t *restrict, const pthread_condattr_t *restrict) = pthread_cond_init;
int main(void) { return 0; }
