#include <aio.h>
void foo(struct aiocb* bar)
{
	struct sigevent *qux = &bar->aio_sigevent;
	(void) qux;
}
int main(void) { return 0; }
