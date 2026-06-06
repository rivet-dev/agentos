#include <netdb.h>
void foo(struct addrinfo* bar)
{
	struct sockaddr **qux = &bar->ai_addr;
	(void) qux;
}
int main(void) { return 0; }
