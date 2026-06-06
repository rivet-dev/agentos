#include <netinet/in.h>
void foo(struct sockaddr_in* bar)
{
	sa_family_t *qux = &bar->sin_family;
	(void) qux;
}
int main(void) { return 0; }
