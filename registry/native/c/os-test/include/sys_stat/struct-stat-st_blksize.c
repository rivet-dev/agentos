/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/stat.h>
void foo(struct stat* bar)
{
	blksize_t *qux = &bar->st_blksize;
	(void) qux;
}
int main(void) { return 0; }
