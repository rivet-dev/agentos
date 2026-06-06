/* Test whether the /dev/stdin file exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/dev/stdin";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
