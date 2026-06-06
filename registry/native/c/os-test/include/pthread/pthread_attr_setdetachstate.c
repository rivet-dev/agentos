#include <pthread.h>
#ifdef pthread_attr_setdetachstate
#undef pthread_attr_setdetachstate
#endif
int (*foo)(pthread_attr_t *, int) = pthread_attr_setdetachstate;
int main(void) { return 0; }
