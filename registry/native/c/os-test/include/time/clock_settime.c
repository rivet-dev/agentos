#include <time.h>
#ifdef clock_settime
#undef clock_settime
#endif
int (*foo)(clockid_t, const struct timespec *) = clock_settime;
int main(void) { return 0; }
