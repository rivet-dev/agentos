#include <netdb.h>
void foo(struct netent* bar)
{
	uint32_t *qux = &bar->n_net;
	(void) qux;
}
int main(void) { return 0; }
