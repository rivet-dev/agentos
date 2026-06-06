#include <aio.h>
#ifdef aio_error
#undef aio_error
#endif
int (*foo)(const struct aiocb *) = aio_error;
int main(void) { return 0; }
