#include <glob.h>
void foo(glob_t* bar)
{
	size_t *qux = &bar->gl_pathc;
	(void) qux;
}
int main(void) { return 0; }
