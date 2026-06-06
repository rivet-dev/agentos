#include <netdb.h>
void foo(struct netent* bar)
{
	char **qux = &bar->n_name;
	(void) qux;
}
int main(void) { return 0; }
