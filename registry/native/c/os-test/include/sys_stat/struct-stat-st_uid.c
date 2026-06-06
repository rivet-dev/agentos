#include <sys/stat.h>
void foo(struct stat* bar)
{
	uid_t *qux = &bar->st_uid;
	(void) qux;
}
int main(void) { return 0; }
