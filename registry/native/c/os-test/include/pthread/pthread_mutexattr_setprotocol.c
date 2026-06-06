/*[MC1]*/
#include <pthread.h>
#ifdef pthread_mutexattr_setprotocol
#undef pthread_mutexattr_setprotocol
#endif
int (*foo)(pthread_mutexattr_t *, int) = pthread_mutexattr_setprotocol;
int main(void) { return 0; }
