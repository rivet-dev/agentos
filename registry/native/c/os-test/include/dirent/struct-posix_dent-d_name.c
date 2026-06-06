#include <dirent.h>
void foo(struct posix_dent* bar)
{
	char *qux = bar->d_name;
	(void) qux;
}
int main(void) { return 0; }
