#include <stdlib.h>
void foo(lldiv_t* bar)
{
	long long *qux = &bar->rem;
	(void) qux;
}
int main(void) { return 0; }
