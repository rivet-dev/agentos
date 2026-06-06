#include <sys/socket.h>
void foo(struct sockaddr* bar)
{
	char *qux = bar->sa_data;
	(void) qux;
}
int main(void) { return 0; }
