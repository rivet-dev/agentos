#include <glob.h>
void foo(glob_t* bar)
{
	char ***qux = &bar->gl_pathv;
	(void) qux;
}
int main(void) { return 0; }
