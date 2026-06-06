/* Test whether a basic rewinddir invocation works. */

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
	bool found_again = false;
	struct dirent* entry;
	while ( (errno = 0, entry = readdir(dir)) )
	{
		if ( !strcmp(entry->d_name, "dirent") )
		{
			if ( !found )
			{
				found = true;
				rewinddir(dir);
			}
			else
				found_again = true;
		}
	}
	if ( errno )
		err(1, "readdir");
	if ( !found )
		errx(1, "did not find dirent subdirectory");
	if ( !found_again )
		errx(1, "did not find dirent subdirectory again");
	closedir(dir);
	return 0;
}
