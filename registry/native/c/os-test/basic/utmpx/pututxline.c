/*[XSI]*/
/* Test whether a basic pututxline invocation works. */

#include <sys/time.h>

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <pwd.h>
#include <string.h>
#include <unistd.h>
#include <utmpx.h>

#include "../basic.h"

int main(void)
{
	struct utmpx entry =
	{
		.ut_pid = getpid(),
		.ut_type = USER_PROCESS,
		.ut_line = "",
	};
	struct passwd* pwd = getpwuid(getuid());
	if ( !pwd )
		err(1, "getpwuid");
	strncpy(entry.ut_user, pwd->pw_name, sizeof(entry.ut_user));
	int tty_fd = open("/dev/tty", O_RDONLY);
	if ( 0 <= tty_fd )
	{
#ifdef TTY_NAME_MAX
		char name[TTY_NAME_MAX];
		if ( !ttyname_r(tty_fd, name, sizeof(name)) )
#else
		char* name = ttyname(tty_fd);
		if ( name )
#endif
		{
			if ( strncmp(name, "/dev/", strlen("/dev/")) )
				strncpy(entry.ut_line, name + strlen("/dev/"),
				        sizeof(entry.ut_line));
			else
				strncpy(entry.ut_line, name, sizeof(entry.ut_line));
		}
	}
	struct timeval now;
	gettimeofday(&now, NULL);
	entry.ut_tv.tv_sec = now.tv_sec;
	entry.ut_tv.tv_usec = now.tv_usec;
	errno = 0;
	// utmp_update may write to stderr on some BSD systems. Avoid that.
	int dev_null = open("/dev/null", O_WRONLY);
	if ( dev_null < 0 )
		err(1, "/dev/null");
	int errfd = dup(2);
	if ( errfd < 0 )
		err(1, "dup");
	dup2(dev_null, 2);
	// de facto: POSIX requires EPERM but a lot of systems fail with EACCES
	// which is also a reasonable error code.
	struct utmpx* result = pututxline(&entry);
	dup2(errfd, 2);
	if ( !result && errno != EPERM && errno != EACCES )
		err(1, "pututxline");
	return 0;
}
