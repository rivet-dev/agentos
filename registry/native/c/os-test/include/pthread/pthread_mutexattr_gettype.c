#include <pthread.h>
#ifdef pthread_mutexattr_gettype
#undef pthread_mutexattr_gettype
#endif
int (*foo)(const pthread_mutexattr_t *restrict, int *restrict) = pthread_mutexattr_gettype;
int main(void) { return 0; }
