#include <sys/stat.h>
void foo(struct stat* bar)
{
	mode_t *qux = &bar->st_mode;
	(void) qux;
}
int main(void) { return 0; }
