#include <pthread.h>
#ifdef pthread_cond_broadcast
#undef pthread_cond_broadcast
#endif
int (*foo)(pthread_cond_t *) = pthread_cond_broadcast;
int main(void) { return 0; }
