#include <stdlib.h>
void foo(div_t* bar)
{
	int *qux = &bar->rem;
	(void) qux;
}
int main(void) { return 0; }
