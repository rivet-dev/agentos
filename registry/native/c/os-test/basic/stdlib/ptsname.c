/*[XSI]*/
/* Test whether a basic ptsname invocation works. */

#include <fcntl.h>
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
	char* name = ptsname(controller);
	if ( !name )
		err(1, "ptsname");
	if ( name[0] != '/' )
		errx(1, "ptsname did not produce absolute path");
	return 0;
}
