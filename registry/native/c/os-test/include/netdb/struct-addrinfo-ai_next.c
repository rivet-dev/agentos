#include <netdb.h>
void foo(struct addrinfo* bar)
{
	struct addrinfo **qux = &bar->ai_next;
	(void) qux;
}
int main(void) { return 0; }
