/*[TSH]*/
#include <pthread.h>
#ifdef pthread_rwlockattr_setpshared
#undef pthread_rwlockattr_setpshared
#endif
int (*foo)(pthread_rwlockattr_t *, int) = pthread_rwlockattr_setpshared;
int main(void) { return 0; }
