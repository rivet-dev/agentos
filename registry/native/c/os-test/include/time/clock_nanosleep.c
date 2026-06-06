#include <time.h>
#ifdef clock_nanosleep
#undef clock_nanosleep
#endif
int (*foo)(clockid_t, int, const struct timespec *, struct timespec *) = clock_nanosleep;
int main(void) { return 0; }
