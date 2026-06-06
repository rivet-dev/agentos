/* Test whether a basic dlopen invocation works. */

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
	// TODO: Does POSIX actually require shared libraries to work?
	void* lib = dlopen("dlfcn/dlopen.so", RTLD_GLOBAL | RTLD_NOW);
	if ( !lib )
		errx(1, "dlopen: %s", dlerror());
	return 0;
}
#endif
