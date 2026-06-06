/*[XSI]*/
/* Test whether a basic closelog invocation works. */

#include <errno.h>
#include <syslog.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	alarm(1);
	errno = 0;
	openlog("os-test", LOG_NDELAY, LOG_USER);
	// Ensure closelog is invoked even if EACCES to ensure coverage.
	if ( errno && errno != EACCES )
		err(1, "openlog");
	closelog();
	return 0;
}
