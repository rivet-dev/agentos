#include <signal.h>
void foo(union sigval* bar)
{
	int *qux = &bar->sival_int;
	(void) qux;
}
int main(void) { return 0; }
