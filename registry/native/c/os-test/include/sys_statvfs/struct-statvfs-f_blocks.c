#include <sys/statvfs.h>
void foo(struct statvfs* bar)
{
	fsblkcnt_t *qux = &bar->f_blocks;
	(void) qux;
}
int main(void) { return 0; }
