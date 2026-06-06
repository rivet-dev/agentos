#include <fcntl.h>
void foo(struct flock* bar)
{
	short *qux = &bar->l_type;
	(void) qux;
}
int main(void) { return 0; }
