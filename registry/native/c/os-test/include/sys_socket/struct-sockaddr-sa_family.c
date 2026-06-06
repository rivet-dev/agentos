#include <sys/socket.h>
void foo(struct sockaddr* bar)
{
	sa_family_t *qux = &bar->sa_family;
	(void) qux;
}
int main(void) { return 0; }
