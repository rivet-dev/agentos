#include <signal.h>
void foo(struct sigevent* bar)
{
	void (**qux)(union sigval) = &bar->sigev_notify_function;
	(void) qux;
}
int main(void) { return 0; }
