#include <sys/socket.h>
void foo(struct cmsghdr* bar)
{
	socklen_t *qux = &bar->cmsg_len;
	(void) qux;
}
int main(void) { return 0; }
