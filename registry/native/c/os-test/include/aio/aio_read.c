#include <aio.h>
#ifdef aio_read
#undef aio_read
#endif
int (*foo)(struct aiocb *) = aio_read;
int main(void) { return 0; }
