/*[IP6]*/
#include <netinet/in.h>
void foo(struct sockaddr_in6* bar)
{
	sa_family_t *qux = &bar->sin6_family;
	(void) qux;
}
int main(void) { return 0; }
