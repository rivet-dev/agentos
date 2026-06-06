/* Test whether a basic pwrite invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	int fd = fileno(fp);
	if ( pwrite(fd, "x", 1, 1) < 1 )
		err(1, "pwrite");
	if ( lseek(fd, 0, SEEK_SET) )
		err(1, "lseek");
	char c;
	if ( read(fd, &c, 1) < 1 )
		err(fd, "first read");
	if ( c != 0 )
		err(fd, "first read did not return 0");
	if ( read(fd, &c, 1) < 1 )
		err(fd, "second read");
	if ( c != 'x' )
		err(fd, "second read did not return x");
	return 0;
}
