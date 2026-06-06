#include <netdb.h>
void foo(struct addrinfo* bar)
{
	socklen_t *qux = &bar->ai_addrlen;
	(void) qux;
}
int main(void) { return 0; }
