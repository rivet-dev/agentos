#include <sys/socket.h>
void foo(struct msghdr* bar)
{
	int *qux = &bar->msg_flags;
	(void) qux;
}
int main(void) { return 0; }
