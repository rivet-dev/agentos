#include <sys/stat.h>
void foo(struct stat* bar)
{
	nlink_t *qux = &bar->st_nlink;
	(void) qux;
}
int main(void) { return 0; }
