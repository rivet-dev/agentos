#include <net/if.h>
void foo(struct if_nameindex* bar)
{
	char **qux = &bar->if_name;
	(void) qux;
}
int main(void) { return 0; }
