#include <netdb.h>
void foo(struct addrinfo* bar)
{
	int *qux = &bar->ai_protocol;
	(void) qux;
}
int main(void) { return 0; }
