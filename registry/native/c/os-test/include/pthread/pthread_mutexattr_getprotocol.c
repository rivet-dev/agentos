/*[MC1]*/
#include <pthread.h>
#ifdef pthread_mutexattr_getprotocol
#undef pthread_mutexattr_getprotocol
#endif
int (*foo)(const pthread_mutexattr_t *restrict, int *restrict) = pthread_mutexattr_getprotocol;
int main(void) { return 0; }
