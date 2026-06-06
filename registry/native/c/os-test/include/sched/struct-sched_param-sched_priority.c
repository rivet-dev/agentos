#include <sched.h>
void foo(struct sched_param* bar)
{
	int *qux = &bar->sched_priority;
	(void) qux;
}
int main(void) { return 0; }
