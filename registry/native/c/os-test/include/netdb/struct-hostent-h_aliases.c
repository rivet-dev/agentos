#include <netdb.h>
void foo(struct hostent* bar)
{
	char ***qux = &bar->h_aliases;
	(void) qux;
}
int main(void) { return 0; }
