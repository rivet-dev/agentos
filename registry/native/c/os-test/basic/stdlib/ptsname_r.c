/*[XSI]*/
/* Test whether a basic ptsname_r invocation works. */

#include <fcntl.h>
#include <limits.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int controller = posix_openpt(O_RDWR | O_NOCTTY);
	if ( controller < 0 )
		err(1, "posix_openpt");
	if ( grantpt(controller) < 0 )
		err(1, "grantpt");
	if ( unlockpt(controller) < 0 )
		err(1, "unlockpt");
#ifdef TTY_NAME_MAX
	char name[TTY_NAME_MAX];
#else
	char name[64];
#endif
	int errnum = ptsname_r(controller, name, sizeof(name));
	if ( errnum )
	{
		errno = errnum;
		err(1, "ptsname_r");
	}
	if ( name[0] != '/' )
		errx(1, "ptsname_r did not produce absolute path");
	return 0;
}
