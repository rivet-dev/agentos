/*[RPP|TPP]*/
#include <pthread.h>
#ifdef pthread_mutexattr_getprioceiling
#undef pthread_mutexattr_getprioceiling
#endif
int (*foo)( const pthread_mutexattr_t *restrict, int *restrict) = pthread_mutexattr_getprioceiling;
int main(void) { return 0; }
