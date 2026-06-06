/*[IP6]*/
#include <netinet/in.h>
void foo(struct ipv6_mreq* bar)
{
	unsigned *qux = &bar->ipv6mr_interface;
	(void) qux;
}
int main(void) { return 0; }
