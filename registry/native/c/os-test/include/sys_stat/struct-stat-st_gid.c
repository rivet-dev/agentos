#include <sys/stat.h>
void foo(struct stat* bar)
{
	gid_t *qux = &bar->st_gid;
	(void) qux;
}
int main(void) { return 0; }
