/*[IP6]*/
#include <netinet/in.h>
void foo(struct sockaddr_in6* bar)
{
	uint32_t *qux = &bar->sin6_scope_id;
	(void) qux;
}
int main(void) { return 0; }
