#include <threads.h>
#ifdef cnd_timedwait
#undef cnd_timedwait
#endif
int (*foo)(cnd_t *restrict, mtx_t *restrict, const struct timespec *restrict) = cnd_timedwait;
int main(void) { return 0; }
