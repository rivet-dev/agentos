/*[XSI]*/
/* Test whether a basic setutxent invocation works. */

#include <errno.h>
#include <utmpx.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	setutxent();
	if ( errno )
		err(1, "setutxent");
	return 0;
}
