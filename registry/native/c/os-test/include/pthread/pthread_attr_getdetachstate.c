#include <pthread.h>
#ifdef pthread_attr_getdetachstate
#undef pthread_attr_getdetachstate
#endif
int (*foo)(const pthread_attr_t *, int *) = pthread_attr_getdetachstate;
int main(void) { return 0; }
