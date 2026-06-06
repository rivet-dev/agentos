#include <netdb.h>
void foo(struct protoent* bar)
{
	char ***qux = &bar->p_aliases;
	(void) qux;
}
int main(void) { return 0; }
