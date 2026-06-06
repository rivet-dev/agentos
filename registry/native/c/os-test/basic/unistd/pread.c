/* Test whether a basic pread invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	int fd = fileno(fp);
	if ( write(fd, "x", 1) < 1 )
		err(1, "first write");
	if ( write(fd, "y", 1) < 1 )
		err(1, "second write");
	if ( lseek(fd, 0, SEEK_SET) )
		err(1, "lseek");
	char c;
	if ( pread(fd, &c, 1, 1) < 1 )
		err(fd, "pread");
	if ( c != 'y' )
		err(fd, "pread read did not return y");
	return 0;
}
