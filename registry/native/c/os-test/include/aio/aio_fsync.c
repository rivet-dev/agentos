/*[FSC|SIO]*/
#include <aio.h>
#ifdef aio_fsync
#undef aio_fsync
#endif
int (*foo)(int, struct aiocb *) = aio_fsync;
int main(void) { return 0; }
