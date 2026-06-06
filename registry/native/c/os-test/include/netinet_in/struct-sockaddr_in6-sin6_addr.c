/*[IP6]*/
#include <netinet/in.h>
void foo(struct sockaddr_in6* bar)
{
	struct in6_addr *qux = &bar->sin6_addr;
	(void) qux;
}
int main(void) { return 0; }
