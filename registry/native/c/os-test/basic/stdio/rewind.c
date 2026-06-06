/* Test whether a basic rewind invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputs("foo", fp) == EOF )
		err(1, "fputs");
	if ( ftell(fp) != 3 )
		errx(1, "first ftell did not return 3");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	if ( ftell(fp) != 0 )
		errx(1, "second ftell did not return 0");
	return 0;
}
