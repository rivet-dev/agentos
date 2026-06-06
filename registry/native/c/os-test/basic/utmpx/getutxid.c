/*[XSI]*/
/* Test whether a basic getutxid invocation works. */

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
	struct utmpx in = { .ut_type = BOOT_TIME };
	struct utmpx* data = getutxid(&in);
	if ( !data && errno )
		err(1, "getutxid");
	return 0;
}
