/*[IP6]*/
#include <netinet/in.h>
void foo(struct in6_addr* bar)
{
	uint8_t *qux = bar->s6_addr;
	(void) qux;
}
int main(void) { return 0; }
