#include <sys/statvfs.h>
void foo(struct statvfs* bar)
{
	unsigned long *qux = &bar->f_bsize;
	(void) qux;
}
int main(void) { return 0; }
