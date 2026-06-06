/* Test whether the /dev/stderr file exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/dev/stderr";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
