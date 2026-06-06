#include <netdb.h>
void foo(struct hostent* bar)
{
	int *qux = &bar->h_length;
	(void) qux;
}
int main(void) { return 0; }
