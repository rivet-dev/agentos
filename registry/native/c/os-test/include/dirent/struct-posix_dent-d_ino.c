#include <dirent.h>
void foo(struct posix_dent* bar)
{
	ino_t *qux = &bar->d_ino;
	(void) qux;
}
int main(void) { return 0; }
