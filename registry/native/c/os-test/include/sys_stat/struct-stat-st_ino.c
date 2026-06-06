#include <sys/stat.h>
void foo(struct stat* bar)
{
	ino_t *qux = &bar->st_ino;
	(void) qux;
}
int main(void) { return 0; }
