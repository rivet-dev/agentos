/* Test whether a basic fileno invocation works. */

#include <sys/stat.h>

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	int fd = fileno(fp);
	if ( fd < 0 )
		err(1, "fileno");
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		err(1, "fstat");
	return 0;
}
