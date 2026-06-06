#include <signal.h>
void foo(siginfo_t* bar)
{
	pid_t *qux = &bar->si_pid;
	(void) qux;
}
int main(void) { return 0; }
