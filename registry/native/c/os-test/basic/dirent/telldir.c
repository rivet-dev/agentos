/*[XSI]*/
/* Test whether a basic telldir invocation works. */

#include <dirent.h>
#include <errno.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	DIR* dir = opendir(".");
	if ( !dir )
		err(1, "opendir");
	// de facto: POSIX doesn't technically say DIR positions are non-negative,
	// but it does forbid negative offsets in lseek per EINVAL. The spirit of
	// Unix is that file offsets are non-negative, so forbid it here. If any
	// such implementations show up, we can amend the test, but good luck with
	// that behavior and compatibility with the software ecosystem.
	long last = telldir(dir);
	if ( last < 0 )
		errx(1, "telldir() < 0");
	struct dirent* entry;
	while ( (errno = 0, entry = readdir(dir)) )
	{
		long now = telldir(dir);
		if ( now < 0 )
			errx(1, "loop telldir() < 0");
		if ( now == last )
			errx(1, "loop now == last");
		last = now;
	}
	if ( errno )
		err(1, "readdir");
	long now = telldir(dir);
	if ( now < 0 )
		errx(1, "afterwards telldir() < 0");
	closedir(dir);
	return 0;
}
