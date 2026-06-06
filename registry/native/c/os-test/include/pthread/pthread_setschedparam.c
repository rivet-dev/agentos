/*[TPS]*/
#include <pthread.h>
#ifdef pthread_setschedparam
#undef pthread_setschedparam
#endif
int (*foo)(pthread_t, int, const struct sched_param *) = pthread_setschedparam;
int main(void) { return 0; }
