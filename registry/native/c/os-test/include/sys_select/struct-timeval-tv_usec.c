#include <sys/select.h>
void foo(struct timeval* bar)
{
	suseconds_t *qux = &bar->tv_usec;
	(void) qux;
}
int main(void) { return 0; }
