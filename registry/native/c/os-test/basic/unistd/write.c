/* Test whether a basic write invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	int fd = fileno(fp);
	char c = 'x';
	if ( write(fd, &c, 1) < 1 )
		err(1, "write");
	if ( lseek(fd, 0, SEEK_SET) < 0 )
		err(1, "lseek");
	c = 'y';
	if ( read(fd, &c, 1) < 1 )
		err(1, "read");
	if ( c != 'x' )
		err(1, "read did not get x");
	return 0;
}
