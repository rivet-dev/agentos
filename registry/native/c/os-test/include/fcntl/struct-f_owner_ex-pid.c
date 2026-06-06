#include <fcntl.h>
void foo(struct f_owner_ex* bar)
{
	pid_t *qux = &bar->pid;
	(void) qux;
}
int main(void) { return 0; }
