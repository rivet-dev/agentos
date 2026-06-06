#include <sys/socket.h>
void foo(struct cmsghdr* bar)
{
	int *qux = &bar->cmsg_level;
	(void) qux;
}
int main(void) { return 0; }
