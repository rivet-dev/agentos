#include <poll.h>
void foo(struct pollfd* bar)
{
	short *qux = &bar->revents;
	(void) qux;
}
int main(void) { return 0; }
