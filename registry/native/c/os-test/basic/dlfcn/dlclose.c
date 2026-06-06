/* Test whether a basic dlclose invocation works. */

#ifdef SHARED
int foo(int value)
{
	return value + 1;
}
#else
#include <dlfcn.h>

#include "../basic.h"

int main(void)
{
	void* lib = dlopen("dlfcn/dlclose.so", RTLD_GLOBAL | RTLD_NOW);
	if ( !lib )
		errx(1, "dlopen: %s", dlerror());
	if ( dlclose(lib) < 0 )
		err(1, "dlclose");
	return 0;
}
#endif
