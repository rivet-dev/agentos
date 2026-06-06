#include <netdb.h>
void foo(struct hostent* bar)
{
	char **qux = &bar->h_name;
	(void) qux;
}
int main(void) { return 0; }
