#include <sys/socket.h>
void foo(struct msghdr* bar)
{
	socklen_t *qux = &bar->msg_controllen;
	(void) qux;
}
int main(void) { return 0; }
