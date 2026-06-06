#include <pthread.h>
#ifdef pthread_mutexattr_settype
#undef pthread_mutexattr_settype
#endif
int (*foo)(pthread_mutexattr_t *, int) = pthread_mutexattr_settype;
int main(void) { return 0; }
