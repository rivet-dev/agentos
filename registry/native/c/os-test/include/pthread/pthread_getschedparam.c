/*[TPS]*/
#include <pthread.h>
#ifdef pthread_getschedparam
#undef pthread_getschedparam
#endif
int (*foo)(pthread_t, int *restrict, struct sched_param *restrict) = pthread_getschedparam;
int main(void) { return 0; }
