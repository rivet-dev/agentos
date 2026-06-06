#include <sys/select.h>
void foo(struct timeval* bar)
{
	time_t *qux = &bar->tv_sec;
	(void) qux;
}
int main(void) { return 0; }
