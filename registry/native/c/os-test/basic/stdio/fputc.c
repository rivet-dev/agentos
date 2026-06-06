/* Test whether a basic fputc invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	int c = 'x';
	if ( fputc(c, fp) == EOF )
		err(1, "fputc");
	if ( fflush(fp) == EOF )
		err(1, "fflush");
	return 0;
}
