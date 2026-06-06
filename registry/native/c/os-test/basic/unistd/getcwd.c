/* Test whether a basic getcwd invocation works. */

#include <limits.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
#ifdef PATH_MAX
	char buf[PATH_MAX];
#else
	char buf[4096];
#endif
	char* result = getcwd(buf, sizeof(buf));
	if ( !result )
		err(1, "getcwd");
	if ( result != buf )
		errx(1, "getcwd did not return buf");
	if ( result[0] != '/' )
		errx(1, "cwd is not absolute");
	return 0;
}
