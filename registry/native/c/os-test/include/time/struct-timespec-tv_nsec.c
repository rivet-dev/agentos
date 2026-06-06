#include <time.h>
void foo(struct timespec* bar)
{
	long *qux = &bar->tv_nsec;
	(void) qux;
}
int main(void) { return 0; }
