#include <signal.h>
void foo(ucontext_t* bar)
{
	mcontext_t *qux = &bar->uc_mcontext;
	(void) qux;
}
int main(void) { return 0; }
