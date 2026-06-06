#include <sys/resource.h>
void foo(struct rlimit* bar)
{
	rlim_t *qux = &bar->rlim_max;
	(void) qux;
}
int main(void) { return 0; }
