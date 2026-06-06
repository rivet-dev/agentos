#include <sys/socket.h>
void foo(struct msghdr* bar)
{
	void **qux = &bar->msg_control;
	(void) qux;
}
int main(void) { return 0; }
