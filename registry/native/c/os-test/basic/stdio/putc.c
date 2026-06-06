/* Test whether a basic putc invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	int c = 'x';
	if ( putc(c, fp) == EOF )
		err(1, "putc");
	if ( fflush(fp) == EOF )
		err(1, "fflush");
	return 0;
}
