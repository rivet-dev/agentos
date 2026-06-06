#include <time.h>
#ifdef timer_gettime
#undef timer_gettime
#endif
int (*foo)(timer_t, struct itimerspec *) = timer_gettime;
int main(void) { return 0; }
