#include <time.h>
#ifdef timer_settime
#undef timer_settime
#endif
int (*foo)(timer_t, int, const struct itimerspec *restrict, struct itimerspec *restrict) = timer_settime;
int main(void) { return 0; }
