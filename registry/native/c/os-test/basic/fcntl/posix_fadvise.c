/*[ADV]*/
/* Test whether a basic posix_fadvise invocation works. */

#include <fcntl.h>
#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( (errno = posix_fadvise(fileno(fp), 0, 2, POSIX_FADV_SEQUENTIAL)) )
		err(1, "posix_fadvise");
	return 0;
}
