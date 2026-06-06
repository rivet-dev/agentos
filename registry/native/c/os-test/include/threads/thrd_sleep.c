#include <threads.h>
#ifdef thrd_sleep
#undef thrd_sleep
#endif
int (*foo)(const struct timespec *, struct timespec *) = thrd_sleep;
int main(void) { return 0; }
