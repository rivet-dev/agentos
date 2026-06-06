#include <pthread.h>
#ifdef pthread_mutexattr_getrobust
#undef pthread_mutexattr_getrobust
#endif
int (*foo)(const pthread_mutexattr_t *restrict, int *restrict) = pthread_mutexattr_getrobust;
int main(void) { return 0; }
