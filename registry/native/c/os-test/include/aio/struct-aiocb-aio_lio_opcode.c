#include <aio.h>
void foo(struct aiocb* bar)
{
	int *qux = &bar->aio_lio_opcode;
	(void) qux;
}
int main(void) { return 0; }
