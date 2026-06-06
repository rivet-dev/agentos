/* Test whether the /dev/pts directory exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/dev/pts";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
