/*[TPS]*/
#include <pthread.h>
#ifdef pthread_setschedprio
#undef pthread_setschedprio
#endif
int (*foo)(pthread_t, int) = pthread_setschedprio;
int main(void) { return 0; }
