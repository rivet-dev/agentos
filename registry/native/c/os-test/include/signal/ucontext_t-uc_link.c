#include <signal.h>
void foo(ucontext_t* bar)
{
	ucontext_t **qux = &bar->uc_link;
	(void) qux;
}
int main(void) { return 0; }
