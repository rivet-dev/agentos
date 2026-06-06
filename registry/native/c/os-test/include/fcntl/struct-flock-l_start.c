#include <fcntl.h>
void foo(struct flock* bar)
{
	off_t *qux = &bar->l_start;
	(void) qux;
}
int main(void) { return 0; }
