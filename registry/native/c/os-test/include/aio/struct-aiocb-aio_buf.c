#include <aio.h>
void foo(struct aiocb* bar)
{
	volatile void **qux = &bar->aio_buf;
	(void) qux;
}
int main(void) { return 0; }
