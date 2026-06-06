/* Test whether a basic dirfd invocation works. */

#include <sys/stat.h>

#include <dirent.h>

#include "../basic.h"

int main(void)
{
	DIR* dir = opendir(".");
	if ( !dir )
		err(1, "opendir");
	int fd = dirfd(dir);
	if ( fd < 0 )
		err(1, "dirfd");
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		err(1, "fstat");
	if ( !S_ISDIR(st.st_mode) )
		errx(1, ". was not a directory");
	return 0;
}
