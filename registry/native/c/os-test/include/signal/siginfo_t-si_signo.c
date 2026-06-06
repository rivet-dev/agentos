#include <signal.h>
void foo(siginfo_t* bar)
{
	int *qux = &bar->si_signo;
	(void) qux;
}
int main(void) { return 0; }
