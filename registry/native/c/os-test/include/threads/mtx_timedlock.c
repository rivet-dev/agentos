#include <threads.h>
#ifdef mtx_timedlock
#undef mtx_timedlock
#endif
int (*foo)(mtx_t *restrict, const struct timespec *restrict) = mtx_timedlock;
int main(void) { return 0; }
