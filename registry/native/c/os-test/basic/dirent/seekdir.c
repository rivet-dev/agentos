/*[XSI]*/
/* Test whether a basic seekdir invocation works. */

#include <dirent.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	DIR* dir = opendir(".");
	if ( !dir )
		err(1, "opendir");
	long position = -1;
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
		if ( !strcmp(entry->d_name, "dirent") )
			position = last;
		last = telldir(dir);
		if ( last < 0 )
			errx(1, "loop telldir() < 0");
	}
	if ( errno )
		err(1, "readdir");
	if ( position < 0 )
		errx(1, "did not find dirent subdirectory");
	seekdir(dir, position);
	if ( telldir(dir) != position )
		errx(1, "seekdir did not seek");
	if ( !(errno = 0, entry = readdir(dir)) )
	{
		if ( errno )
			errx(1, "readdir");
		errx(1, "unexpected end of directory");
	}
	if ( strcmp(entry->d_name, "dirent") != 0 )
		errx(1, "dirent was not found after seekdir");
	closedir(dir);
	return 0;
}
