#include <netinet/in.h>
void foo(struct sockaddr_in* bar)
{
	struct in_addr *qux = &bar->sin_addr;
	(void) qux;
}
int main(void) { return 0; }
