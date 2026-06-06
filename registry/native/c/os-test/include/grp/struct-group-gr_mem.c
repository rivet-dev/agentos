#include <grp.h>
void foo(struct group* bar)
{
	char ***qux = &bar->gr_mem;
	(void) qux;
}
int main(void) { return 0; }
