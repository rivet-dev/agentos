#include <signal.h>
void foo(stack_t* bar)
{
	size_t *qux = &bar->ss_size;
	(void) qux;
}
int main(void) { return 0; }
