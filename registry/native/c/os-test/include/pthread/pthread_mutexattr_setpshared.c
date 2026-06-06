/*[TSH]*/
#include <pthread.h>
#ifdef pthread_mutexattr_setpshared
#undef pthread_mutexattr_setpshared
#endif
int (*foo)(pthread_mutexattr_t *, int) = pthread_mutexattr_setpshared;
int main(void) { return 0; }
