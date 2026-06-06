/*[IP6]*/
#include <netinet/in.h>
void foo(struct sockaddr_in6* bar)
{
	in_port_t *qux = &bar->sin6_port;
	(void) qux;
}
int main(void) { return 0; }
