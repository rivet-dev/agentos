/*[TSH]*/
#include <pthread.h>
#ifdef pthread_condattr_getpshared
#undef pthread_condattr_getpshared
#endif
int (*foo)(const pthread_condattr_t *restrict, int *restrict) = pthread_condattr_getpshared;
int main(void) { return 0; }
