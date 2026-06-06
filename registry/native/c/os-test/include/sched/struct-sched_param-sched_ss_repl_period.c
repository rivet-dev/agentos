/*[SS|TSP]*/
#include <sched.h>
void foo(struct sched_param* bar)
{
	struct timespec *qux = &bar->sched_ss_repl_period;
	(void) qux;
}
int main(void) { return 0; }
