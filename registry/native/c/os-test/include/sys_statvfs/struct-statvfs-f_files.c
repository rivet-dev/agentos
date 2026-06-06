#include <sys/statvfs.h>
void foo(struct statvfs* bar)
{
	fsfilcnt_t *qux = &bar->f_files;
	(void) qux;
}
int main(void) { return 0; }
