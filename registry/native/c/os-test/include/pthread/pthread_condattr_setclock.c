#include <pthread.h>
#ifdef pthread_condattr_setclock
#undef pthread_condattr_setclock
#endif
int (*foo)(pthread_condattr_t *, clockid_t) = pthread_condattr_setclock;
int main(void) { return 0; }
