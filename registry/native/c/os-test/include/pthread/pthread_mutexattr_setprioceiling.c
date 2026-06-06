/*[RPP|TPP]*/
#include <pthread.h>
#ifdef pthread_mutexattr_setprioceiling
#undef pthread_mutexattr_setprioceiling
#endif
int (*foo)(pthread_mutexattr_t *, int) = pthread_mutexattr_setprioceiling;
int main(void) { return 0; }
