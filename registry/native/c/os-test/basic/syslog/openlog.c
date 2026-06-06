/*[XSI]*/
/* Test whether a basic openlog invocation works. */

#include <errno.h>
#include <syslog.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	openlog("os-test", LOG_ODELAY, LOG_USER);
	if ( errno )
		err(1, "openlog");
	return 0;
}
