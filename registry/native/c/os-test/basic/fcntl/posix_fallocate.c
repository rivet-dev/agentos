/*[ADV]*/
/* Test whether a basic posix_fallocate invocation works. */

#include <sys/stat.h>

#include <fcntl.h>
#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( (errno = posix_fallocate(fileno(fp), 1, 2)) )
		err(1, "posix_fallocate");
	struct stat st;
	if ( fstat(fileno(fp), &st) < 0 )
		err(1, "fstat");
	if ( st.st_size != 3 )
		errx(1, "st_size != 3");
	return 0;
}
