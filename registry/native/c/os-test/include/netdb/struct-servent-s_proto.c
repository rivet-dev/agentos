#include <netdb.h>
void foo(struct servent* bar)
{
	char **qux = &bar->s_proto;
	(void) qux;
}
int main(void) { return 0; }
