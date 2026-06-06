#include <signal.h>
void foo(stack_t* bar)
{
	void **qux = &bar->ss_sp;
	(void) qux;
}
int main(void) { return 0; }
