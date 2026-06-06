#include <fcntl.h>
void foo(struct flock* bar)
{
	off_t *qux = &bar->l_len;
	(void) qux;
}
int main(void) { return 0; }
