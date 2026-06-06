#include <netdb.h>
void foo(struct hostent* bar)
{
	int *qux = &bar->h_addrtype;
	(void) qux;
}
int main(void) { return 0; }
