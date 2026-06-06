#include <time.h>
#ifdef clock_gettime
#undef clock_gettime
#endif
int (*foo)(clockid_t, struct timespec *) = clock_gettime;
int main(void) { return 0; }
