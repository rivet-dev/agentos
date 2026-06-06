#include <sys/stat.h>
void foo(struct stat* bar)
{
	off_t *qux = &bar->st_size;
	(void) qux;
}
int main(void) { return 0; }
