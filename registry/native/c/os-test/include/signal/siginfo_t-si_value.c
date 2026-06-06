#include <signal.h>
void foo(siginfo_t* bar)
{
	union sigval *qux = &bar->si_value;
	(void) qux;
}
int main(void) { return 0; }
