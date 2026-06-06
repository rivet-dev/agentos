#include <sys/stat.h>
void foo(struct stat* bar)
{
	dev_t *qux = &bar->st_dev;
	(void) qux;
}
int main(void) { return 0; }
