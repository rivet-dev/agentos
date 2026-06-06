/*[IP6]*/
#include <netinet/in.h>
void foo(struct ipv6_mreq* bar)
{
	struct in6_addr *qux = &bar->ipv6mr_multiaddr;
	(void) qux;
}
int main(void) { return 0; }
