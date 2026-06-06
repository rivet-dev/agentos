#include <sys/socket.h>
void foo(struct msghdr* bar)
{
	struct iovec **qux = &bar->msg_iov;
	(void) qux;
}
int main(void) { return 0; }
