/*[OB]*/
/* Test whether a basic readdir_r invocation works. */

#include <dirent.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	DIR* dir = opendir(".");
	if ( !dir )
		err(1, "opendir");
	long name_max = fpathconf(dirfd(dir), _PC_NAME_MAX);
	if ( name_max < 0 )
		errx(1, "fpathconf: _PC_NAME_MAX");
	struct dirent* buffer = malloc(sizeof(struct dirent) + name_max + 1);
	if ( !buffer )
		errx(1, "malloc");
	bool found = false;
	struct dirent* entry;
	while ( !(errno = readdir_r(dir, buffer, &entry)) && entry )
	{
		if ( !strcmp(entry->d_name, "dirent") )
			found = true;
	}
	if ( errno )
		err(1, "readdir_r");
	if ( !found )
		errx(1, "did not find dirent subdirectory");
	closedir(dir);
	return 0;
}
