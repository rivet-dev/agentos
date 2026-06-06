/*[XSI]*/
/* Test whether a basic syslog invocation works. */

#include <errno.h>
#include <fcntl.h>
#include <syslog.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	alarm(1);
	errno = 0;
	openlog("os-test", LOG_ODELAY, LOG_USER);
	if ( errno )
		err(1, "openlog");
	errno = 0;
	// "The syslog() function shall send a message to an implementation-defined
	//  logging facility, which may log it in an implementation-defined system
	//  log, write it to the system console, forward it to a list of users, or
	//  forward it to the logging facility on another host over the network."
	// On Sortix, syslog simply writes to stderr. If that happens, a
	// non-deterministic mesage containing a timestmap is written to stderr,
	// which results with results collection. For that reason, we temporarily
	// redirect stderr to /dev/null.
	int dev_null = open("/dev/null", O_WRONLY);
	if ( dev_null < 0 )
		err(1, "/dev/null");
	int old_stderr = dup(2);
	if ( old_stderr < 0 )
		err(1, "dup");
	if ( dup2(dev_null, 2) < 0 )
		err(1, "dup2");
	// There isn't a portable way to know if the message actually went through.
	// But if you see this message in your logs after running os-test, this
	// test did pass.
	syslog(LOG_DEBUG, "os-test is being run");
	// Restore stderr.
	int errnum = errno;
	if ( dup2(old_stderr, 2) < 0 )
		err(1, "dup2");
	errno = errnum;
	if ( errno && errno != EACCES )
		err(1, "syslog");
	return 0;
}
