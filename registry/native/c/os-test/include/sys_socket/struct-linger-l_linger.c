#include <sys/socket.h>
void foo(struct linger* bar)
{
	int *qux = &bar->l_linger;
	(void) qux;
}
int main(void) { return 0; }
