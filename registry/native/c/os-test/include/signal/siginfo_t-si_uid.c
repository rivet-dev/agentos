#include <signal.h>
void foo(siginfo_t* bar)
{
	uid_t *qux = &bar->si_uid;
	(void) qux;
}
int main(void) { return 0; }
