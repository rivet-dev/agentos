#include <aio.h>
#ifdef aio_suspend
#undef aio_suspend
#endif
int (*foo)(const struct aiocb *const [], int, const struct timespec *) = aio_suspend;
int main(void) { return 0; }
