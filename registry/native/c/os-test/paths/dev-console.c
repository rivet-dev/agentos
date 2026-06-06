/* Test whether the /dev/console file exists. */

#include "suite.h"

int main(void)
{
	// POSIX requires /dev/console to exist.
	const char* path = "/dev/console";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
