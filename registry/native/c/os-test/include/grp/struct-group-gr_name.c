#include <grp.h>
void foo(struct group* bar)
{
	char **qux = &bar->gr_name;
	(void) qux;
}
int main(void) { return 0; }
