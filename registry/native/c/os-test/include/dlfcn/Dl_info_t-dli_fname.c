#include <dlfcn.h>
void foo(Dl_info_t* bar)
{
	const char **qux = &bar->dli_fname;
	(void) qux;
}
int main(void) { return 0; }
