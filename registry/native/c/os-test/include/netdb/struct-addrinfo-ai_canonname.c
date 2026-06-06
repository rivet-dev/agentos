#include <netdb.h>
void foo(struct addrinfo* bar)
{
	char **qux = &bar->ai_canonname;
	(void) qux;
}
int main(void) { return 0; }
