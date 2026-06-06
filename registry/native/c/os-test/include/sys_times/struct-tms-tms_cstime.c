#include <sys/times.h>
void foo(struct tms* bar)
{
	clock_t *qux = &bar->tms_cstime;
	(void) qux;
}
int main(void) { return 0; }
