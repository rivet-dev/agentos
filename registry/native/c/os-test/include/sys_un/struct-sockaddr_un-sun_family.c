#include <sys/un.h>
void foo(struct sockaddr_un* bar)
{
	sa_family_t *qux = &bar->sun_family;
	(void) qux;
}
int main(void) { return 0; }
