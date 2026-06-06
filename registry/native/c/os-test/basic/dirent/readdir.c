/* Test whether a basic readdir invocation works. */

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
	bool found = false;
	struct dirent* entry;
	while ( (errno = 0, entry = readdir(dir)) )
	{
		if ( !strcmp(entry->d_name, "dirent") )
			found = true;
	}
	if ( errno )
		err(1, "readdir");
	if ( !found )
		errx(1, "did not find dirent subdirectory");
	closedir(dir);
	return 0;
}
