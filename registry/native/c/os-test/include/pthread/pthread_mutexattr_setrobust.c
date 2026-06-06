#include <pthread.h>
#ifdef pthread_mutexattr_setrobust
#undef pthread_mutexattr_setrobust
#endif
int (*foo)(pthread_mutexattr_t *, int) = pthread_mutexattr_setrobust;
int main(void) { return 0; }
