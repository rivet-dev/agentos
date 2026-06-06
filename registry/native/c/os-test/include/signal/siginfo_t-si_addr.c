#include <signal.h>
void foo(siginfo_t* bar)
{
	void **qux = &bar->si_addr;
	(void) qux;
}
int main(void) { return 0; }
