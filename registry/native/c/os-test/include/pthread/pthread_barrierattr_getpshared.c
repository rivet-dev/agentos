/*[TSH]*/
#include <pthread.h>
#ifdef pthread_barrierattr_getpshared
#undef pthread_barrierattr_getpshared
#endif
int (*foo)( const pthread_barrierattr_t *restrict, int *restrict) = pthread_barrierattr_getpshared;
int main(void) { return 0; }
