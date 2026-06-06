/*[TSH]*/
#include <pthread.h>
#ifdef pthread_barrierattr_setpshared
#undef pthread_barrierattr_setpshared
#endif
int (*foo)(pthread_barrierattr_t *, int) = pthread_barrierattr_setpshared;
int main(void) { return 0; }
