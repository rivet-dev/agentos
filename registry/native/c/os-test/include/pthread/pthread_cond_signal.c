#include <pthread.h>
#ifdef pthread_cond_signal
#undef pthread_cond_signal
#endif
int (*foo)(pthread_cond_t *) = pthread_cond_signal;
int main(void) { return 0; }
