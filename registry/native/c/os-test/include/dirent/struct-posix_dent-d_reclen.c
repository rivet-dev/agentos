#include <dirent.h>
void foo(struct posix_dent* bar)
{
	reclen_t *qux = &bar->d_reclen;
	(void) qux;
}
int main(void) { return 0; }
