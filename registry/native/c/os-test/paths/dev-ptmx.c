/* Test whether the /dev/ptmx file exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/dev/ptmx";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
