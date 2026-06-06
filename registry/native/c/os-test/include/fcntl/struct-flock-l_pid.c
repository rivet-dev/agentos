#include <fcntl.h>
void foo(struct flock* bar)
{
	pid_t *qux = &bar->l_pid;
	(void) qux;
}
int main(void) { return 0; }
