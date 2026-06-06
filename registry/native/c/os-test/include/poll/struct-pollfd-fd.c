#include <poll.h>
void foo(struct pollfd* bar)
{
	int *qux = &bar->fd;
	(void) qux;
}
int main(void) { return 0; }
