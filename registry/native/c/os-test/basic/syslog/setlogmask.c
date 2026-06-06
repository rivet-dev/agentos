/*[XSI]*/
/* Test whether a basic setlogmask invocation works. */

#include <syslog.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	setlogmask(LOG_UPTO(LOG_DEBUG));
	int old = setlogmask(LOG_UPTO(LOG_WARNING));
	if ( old != LOG_UPTO(LOG_DEBUG) )
		errx(1, "setlogmask did not return the old mask");
	alarm(1);
	errno = 0;
	openlog("os-test", LOG_ODELAY, LOG_USER);
	if ( errno )
		err(1, "openlog");
	errno = 0;
	// There isn't a portable way to know if the message actually went through.
	// But if you see this message in your logs after running os-test, this
	// test failed.
	syslog(LOG_DEBUG, "os-test should not write this message");
	if ( errno )
		err(1, "syslog");
	return 0;
}
