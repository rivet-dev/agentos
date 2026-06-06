/*[XSI]*/
/* Test whether a basic unlockpt invocation works. */

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
	return 0;
}
