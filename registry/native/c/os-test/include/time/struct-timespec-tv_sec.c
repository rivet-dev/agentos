#include <time.h>
void foo(struct timespec* bar)
{
	time_t *qux = &bar->tv_sec;
	(void) qux;
}
int main(void) { return 0; }
