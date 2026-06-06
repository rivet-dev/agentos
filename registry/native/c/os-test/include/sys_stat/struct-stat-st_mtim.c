#include <sys/stat.h>
void foo(struct stat* bar)
{
	struct timespec *qux = &bar->st_mtim;
	(void) qux;
}
int main(void) { return 0; }
