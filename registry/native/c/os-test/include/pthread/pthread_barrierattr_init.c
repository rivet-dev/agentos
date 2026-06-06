#include <pthread.h>
#ifdef pthread_barrierattr_init
#undef pthread_barrierattr_init
#endif
int (*foo)(pthread_barrierattr_t *) = pthread_barrierattr_init;
int main(void) { return 0; }
