#include <time.h>
void foo(struct itimerspec* bar)
{
	struct timespec *qux = &bar->it_interval;
	(void) qux;
}
int main(void) { return 0; }
