/* Test whether the /var/log directory exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/var/log";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
