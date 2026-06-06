/* Test whether the /dev/ptc file exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/dev/ptc";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
