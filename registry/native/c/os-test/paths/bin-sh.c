/* Test whether the /bin/sh file exists. */

#include "suite.h"

int main(void)
{
	// /bin/sh is universally defacto Unix even if not standardized.
	const char* path = "/bin/sh";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
