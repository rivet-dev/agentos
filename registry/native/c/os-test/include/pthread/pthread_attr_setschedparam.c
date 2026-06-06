#include <pthread.h>
#ifdef pthread_attr_setschedparam
#undef pthread_attr_setschedparam
#endif
int (*foo)(pthread_attr_t *restrict, const struct sched_param *restrict) = pthread_attr_setschedparam;
int main(void) { return 0; }
