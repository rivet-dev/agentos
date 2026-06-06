/* Test whether the /var/cache directory exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/var/cache";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
