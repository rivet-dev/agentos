#include <signal.h>
void foo(union sigval* bar)
{
	void **qux = &bar->sival_ptr;
	(void) qux;
}
int main(void) { return 0; }
