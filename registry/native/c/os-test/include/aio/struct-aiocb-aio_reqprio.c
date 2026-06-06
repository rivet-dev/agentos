#include <aio.h>
void foo(struct aiocb* bar)
{
	int *qux = &bar->aio_reqprio;
	(void) qux;
}
int main(void) { return 0; }
