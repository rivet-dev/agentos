#include <aio.h>
#ifdef aio_cancel
#undef aio_cancel
#endif
int (*foo)(int, struct aiocb *) = aio_cancel;
int main(void) { return 0; }
