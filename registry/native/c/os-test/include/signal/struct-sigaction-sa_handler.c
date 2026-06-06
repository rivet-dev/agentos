#include <signal.h>
void foo(struct sigaction* bar)
{
	void (**qux)(int) = &bar->sa_handler;
	(void) qux;
}
int main(void) { return 0; }
