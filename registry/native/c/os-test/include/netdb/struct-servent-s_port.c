#include <netdb.h>
void foo(struct servent* bar)
{
	int *qux = &bar->s_port;
	(void) qux;
}
int main(void) { return 0; }
