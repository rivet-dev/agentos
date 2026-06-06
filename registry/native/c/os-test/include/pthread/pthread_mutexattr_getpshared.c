/*[TSH]*/
#include <pthread.h>
#ifdef pthread_mutexattr_getpshared
#undef pthread_mutexattr_getpshared
#endif
int (*foo)(const pthread_mutexattr_t *restrict, int *restrict) = pthread_mutexattr_getpshared;
int main(void) { return 0; }
