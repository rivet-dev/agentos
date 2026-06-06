#include <signal.h>
void foo(stack_t* bar)
{
	int *qux = &bar->ss_flags;
	(void) qux;
}
int main(void) { return 0; }
