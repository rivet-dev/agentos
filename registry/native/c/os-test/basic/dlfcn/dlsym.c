/* Test whether a basic dlsym invocation works. */

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
	void* lib = dlopen("dlfcn/dlsym.so", RTLD_GLOBAL | RTLD_NOW);
	if ( !lib )
		errx(1, "dlopen: %s", dlerror());
	int (*foo_ptr)(int) = dlsym(lib, "foo");
	if ( !foo_ptr )
		errx(1, "dlsym: %s", dlerror());
	int output = foo_ptr(41);
	if ( output != 42 )
		errx(1, "foo symbol did not return 42");
	return 0;
}
#endif
