#include <aio.h>
void foo(struct aiocb* bar)
{
	size_t *qux = &bar->aio_nbytes;
	(void) qux;
}
int main(void) { return 0; }
