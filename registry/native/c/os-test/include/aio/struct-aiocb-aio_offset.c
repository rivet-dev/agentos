#include <aio.h>
void foo(struct aiocb* bar)
{
	off_t *qux = &bar->aio_offset;
	(void) qux;
}
int main(void) { return 0; }
