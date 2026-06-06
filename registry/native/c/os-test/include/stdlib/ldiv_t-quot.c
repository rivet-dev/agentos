#include <stdlib.h>
void foo(ldiv_t* bar)
{
	long *qux = &bar->quot;
	(void) qux;
}
int main(void) { return 0; }
