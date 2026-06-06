/*[TPS]*/
#include <pthread.h>
#ifdef pthread_attr_setschedpolicy
#undef pthread_attr_setschedpolicy
#endif
int (*foo)(pthread_attr_t *, int) = pthread_attr_setschedpolicy;
int main(void) { return 0; }
