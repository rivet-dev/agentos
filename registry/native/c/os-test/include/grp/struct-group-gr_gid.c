#include <grp.h>
void foo(struct group* bar)
{
	gid_t *qux = &bar->gr_gid;
	(void) qux;
}
int main(void) { return 0; }
