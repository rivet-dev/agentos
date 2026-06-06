#include <signal.h>
void foo(struct sigaction* bar)
{
	int *qux = &bar->sa_flags;
	(void) qux;
}
int main(void) { return 0; }
