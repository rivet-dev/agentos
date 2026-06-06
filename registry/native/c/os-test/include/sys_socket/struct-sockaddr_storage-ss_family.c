#include <sys/socket.h>
void foo(struct sockaddr_storage* bar)
{
	sa_family_t *qux = &bar->ss_family;
	(void) qux;
}
int main(void) { return 0; }
