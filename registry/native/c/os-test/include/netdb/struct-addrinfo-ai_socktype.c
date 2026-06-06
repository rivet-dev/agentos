#include <netdb.h>
void foo(struct addrinfo* bar)
{
	int *qux = &bar->ai_socktype;
	(void) qux;
}
int main(void) { return 0; }
