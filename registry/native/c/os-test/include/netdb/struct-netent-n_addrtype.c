#include <netdb.h>
void foo(struct netent* bar)
{
	int *qux = &bar->n_addrtype;
	(void) qux;
}
int main(void) { return 0; }
