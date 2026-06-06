#include <time.h>
#ifdef nanosleep
#undef nanosleep
#endif
int (*foo)(const struct timespec *, struct timespec *) = nanosleep;
int main(void) { return 0; }
