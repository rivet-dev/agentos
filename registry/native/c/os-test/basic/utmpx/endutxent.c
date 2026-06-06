/*[XSI]*/
/* Test whether a basic endutxent invocation works. */

#include <errno.h>
#include <utmpx.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	endutxent();
	if ( errno )
		err(1, "endutxent");

	errno = 0;
	setutxent();
	if ( errno )
		err(1, "setutxent");

	errno = 0;
	endutxent();
	if ( errno )
		err(1, "endutxent");

	return 0;
}
