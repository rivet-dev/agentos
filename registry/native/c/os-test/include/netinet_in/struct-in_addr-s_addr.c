#include <netinet/in.h>
void foo(struct in_addr* bar)
{
	in_addr_t *qux = &bar->s_addr;
	(void) qux;
}
int main(void) { return 0; }
