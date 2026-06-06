/* Test whether the / directory exists. */

#include "suite.h"

int main(void)
{
	// POSIX requires / to exist.
	const char* path = "/";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
