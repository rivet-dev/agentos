#include <dlfcn.h>
void foo(Dl_info_t* bar)
{
	void **qux = &bar->dli_fbase;
	(void) qux;
}
int main(void) { return 0; }
