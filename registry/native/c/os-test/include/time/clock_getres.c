#include <time.h>
#ifdef clock_getres
#undef clock_getres
#endif
int (*foo)(clockid_t, struct timespec *) = clock_getres;
int main(void) { return 0; }
