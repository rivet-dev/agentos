#include <netinet/in.h>
void foo(struct sockaddr_in* bar)
{
	in_port_t *qux = &bar->sin_port;
	(void) qux;
}
int main(void) { return 0; }
