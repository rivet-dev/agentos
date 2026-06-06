#include <fcntl.h>
void foo(struct flock* bar)
{
	short *qux = &bar->l_whence;
	(void) qux;
}
int main(void) { return 0; }
