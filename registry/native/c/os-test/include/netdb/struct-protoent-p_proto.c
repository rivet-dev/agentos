#include <netdb.h>
void foo(struct protoent* bar)
{
	int *qux = &bar->p_proto;
	(void) qux;
}
int main(void) { return 0; }
