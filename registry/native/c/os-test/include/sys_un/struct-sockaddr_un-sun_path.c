#include <sys/un.h>
void foo(struct sockaddr_un* bar)
{
	char *qux = bar->sun_path;
	(void) qux;
}
int main(void) { return 0; }
