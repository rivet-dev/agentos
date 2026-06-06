/*[SS|TSP]*/
#include <sched.h>
void foo(struct sched_param* bar)
{
	int *qux = &bar->sched_ss_max_repl;
	(void) qux;
}
int main(void) { return 0; }
