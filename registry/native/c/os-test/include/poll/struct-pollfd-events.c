#include <poll.h>
void foo(struct pollfd* bar)
{
	short *qux = &bar->events;
	(void) qux;
}
int main(void) { return 0; }
