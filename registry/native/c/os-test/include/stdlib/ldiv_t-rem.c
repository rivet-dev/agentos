#include <stdlib.h>
void foo(ldiv_t* bar)
{
	long *qux = &bar->rem;
	(void) qux;
}
int main(void) { return 0; }
