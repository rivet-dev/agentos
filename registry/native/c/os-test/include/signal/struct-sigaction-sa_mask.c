#include <signal.h>
void foo(struct sigaction* bar)
{
	sigset_t *qux = &bar->sa_mask;
	(void) qux;
}
int main(void) { return 0; }
