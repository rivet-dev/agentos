/* Test whether a basic fdopendir invocation works. */

#include <sys/stat.h>

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	int fd = open(".", O_RDONLY | O_DIRECTORY);
	if ( fd < 0 )
		err(1, "open: .");
	DIR* dir = fdopendir(fd);
	if ( !dir )
		err(1, "fdopendir");
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
	struct stat st;
	if ( fstat(fd, &st) < 0 )
	{
		if ( errno != EBADF )
			err(1, "closedir didn't close the fd");
	}
	else
		errx(1, "closedir didn't close the fd");
	return 0;
}
