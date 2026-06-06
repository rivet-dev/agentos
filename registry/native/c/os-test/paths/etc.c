/* Test whether the /etc directory exists. */

#include "suite.h"

int main(void)
{
	const char* path = "/etc";
	if ( access(path, F_OK) < 0 )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
