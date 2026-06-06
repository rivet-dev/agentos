#include <pthread.h>
#ifdef pthread_barrierattr_destroy
#undef pthread_barrierattr_destroy
#endif
int (*foo)(pthread_barrierattr_t *) = pthread_barrierattr_destroy;
int main(void) { return 0; }
