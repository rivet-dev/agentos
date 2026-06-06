#include <netdb.h>
void foo(struct netent* bar)
{
	char ***qux = &bar->n_aliases;
	(void) qux;
}
int main(void) { return 0; }
