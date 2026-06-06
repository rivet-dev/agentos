#include <sys/times.h>
void foo(struct tms* bar)
{
	clock_t *qux = &bar->tms_utime;
	(void) qux;
}
int main(void) { return 0; }
