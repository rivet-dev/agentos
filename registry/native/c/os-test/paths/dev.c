/* Test whether the /dev directory exists. */

#include "suite.h"

int main(void)
{
	// POSIX requires /dev to exist.
	const char* path = "/dev";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
