/*[TSS]*/
#include <pthread.h>
#ifdef pthread_attr_getstacksize
#undef pthread_attr_getstacksize
#endif
int (*foo)(const pthread_attr_t *restrict, size_t *restrict) = pthread_attr_getstacksize;
int main(void) { return 0; }
