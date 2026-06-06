#include <pthread.h>
#ifdef pthread_attr_getschedparam
#undef pthread_attr_getschedparam
#endif
int (*foo)(const pthread_attr_t *restrict, struct sched_param *restrict) = pthread_attr_getschedparam;
int main(void) { return 0; }
