/* Test whether a basic feof invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fgetc(fp) != EOF )
		errx(1, "fgetc succeeded on empty file");
	if ( !feof(fp) )
		errx(1, "feof did not have eof condition");
	return 0;
}
