/* Test whether a basic posix_getdents invocation works. */

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fd = open(".", O_RDONLY | O_DIRECTORY);
	if ( fd < 0 )
		err(1, "open: .");
	long name_max = fpathconf(fd, _PC_NAME_MAX);
	if ( name_max < 0 )
		errx(1, "fpathconf: _PC_NAME_MAX");
	size_t size = sizeof(struct posix_dent) + name_max + 1;
	char* buffer = malloc(size);
	if ( !buffer )
		errx(1, "malloc");
	bool found = false;
	ssize_t amount;
	while ( 0 < (amount = posix_getdents(fd, buffer, size, 0)) )
	{
		ssize_t offset = 0;
		while ( offset < amount )
		{
			struct posix_dent* entry = (struct posix_dent*) (buffer + offset);
			if ( !strcmp(entry->d_name, "dirent") )
				found = true;
			offset += entry->d_reclen;
		}
	}
	if ( errno )
		err(1, "readdir");
	if ( !found )
		errx(1, "did not find dirent subdirectory");
	return 0;
}
