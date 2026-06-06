#include <aio.h>
#ifdef aio_return
#undef aio_return
#endif
ssize_t (*foo)(struct aiocb *) = aio_return;
int main(void) { return 0; }
