/* Test whether a basic dlerror invocation works. */

#include <dlfcn.h>

#include "../basic.h"

int main(void)
{
	void* lib = dlopen("dlfcn/dlerror.so", RTLD_GLOBAL | RTLD_NOW);
	if ( !lib )
	{
		char* error = dlerror();
		if ( !error[0] )
			errx(1, "dlerror empty error");
	}
	else
		errx(1, "dlopen did not fail");
	return 0;
}
