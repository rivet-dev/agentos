/* Test whether a basic ftruncate invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( ftruncate(fileno(fp), 42) < 0 )
		err(1, "ftruncate");
	off_t size = lseek(fileno(fp), 0, SEEK_END);
	if ( size < 0 )
		err(1, "lseek");
	if ( size != 42 )
		errx(1, "wrong size");
	return 0;
}
