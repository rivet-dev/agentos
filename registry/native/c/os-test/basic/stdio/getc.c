/* Test whether a basic getc invocation works. */

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
	if ( fseek(fp, 0, SEEK_SET) )
		err(1, "fseek");
	int x = getc(fp);
	if ( x == EOF )
	{
		if ( feof(fp) )
			errx(1, "getc: EOF");
		err(1, "getc");
	}
	if ( c != x )
		errx(1, "getc got %c instead of %c", x, c);
	return 0;
}
