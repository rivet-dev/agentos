#include <signal.h>
void foo(struct sigevent* bar)
{
	union sigval *qux = &bar->sigev_value;
	(void) qux;
}
int main(void) { return 0; }
