/*[TPS]*/
#include <pthread.h>
#ifdef pthread_attr_getschedpolicy
#undef pthread_attr_getschedpolicy
#endif
int (*foo)(const pthread_attr_t *restrict, int *restrict) = pthread_attr_getschedpolicy;
int main(void) { return 0; }
