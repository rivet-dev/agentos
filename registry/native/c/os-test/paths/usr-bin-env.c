/* Test whether the /usr/bin/env file exists. */

#include "suite.h"

int main(void)
{
	// /usr/bin/sh is universally defacto Unix even if not standardized.
	const char* path = "/usr/bin/env";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
