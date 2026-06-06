/*[TSH]*/
#include <pthread.h>
#ifdef pthread_rwlockattr_getpshared
#undef pthread_rwlockattr_getpshared
#endif
int (*foo)( const pthread_rwlockattr_t *restrict, int *restrict) = pthread_rwlockattr_getpshared;
int main(void) { return 0; }
