#include <aio.h>
#ifdef aio_write
#undef aio_write
#endif
int (*foo)(struct aiocb *) = aio_write;
int main(void) { return 0; }
