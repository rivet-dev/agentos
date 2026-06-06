#include <netdb.h>
void foo(struct addrinfo* bar)
{
	int *qux = &bar->ai_family;
	(void) qux;
}
int main(void) { return 0; }
