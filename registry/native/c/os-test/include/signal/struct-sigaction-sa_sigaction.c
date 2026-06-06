#include <signal.h>
void foo(struct sigaction* bar)
{
	void (**qux)(int, siginfo_t *, void *) = &bar->sa_sigaction;
	(void) qux;
}
int main(void) { return 0; }
