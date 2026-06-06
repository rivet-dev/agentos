#include <signal.h>
void foo(struct sigevent* bar)
{
	int *qux = &bar->sigev_notify;
	(void) qux;
}
int main(void) { return 0; }
