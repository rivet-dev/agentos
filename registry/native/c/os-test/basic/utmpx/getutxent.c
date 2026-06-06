/*[XSI]*/
/* Test whether a basic getutxent invocation works. */

#include <errno.h>
#include <utmpx.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	setutxent();
	if ( errno )
		err(1, "setutxent");

	errno = 0;
	struct utmpx* data = getutxent();
	if ( !data && errno )
		err(1, "getutxent");
	return 0;
}
