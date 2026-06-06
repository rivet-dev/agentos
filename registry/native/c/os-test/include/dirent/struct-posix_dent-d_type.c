#include <dirent.h>
void foo(struct posix_dent* bar)
{
	unsigned char *qux = &bar->d_type;
	(void) qux;
}
int main(void) { return 0; }
