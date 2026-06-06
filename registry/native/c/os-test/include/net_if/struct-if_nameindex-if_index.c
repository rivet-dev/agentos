#include <net/if.h>
void foo(struct if_nameindex* bar)
{
	unsigned *qux = &bar->if_index;
	(void) qux;
}
int main(void) { return 0; }
