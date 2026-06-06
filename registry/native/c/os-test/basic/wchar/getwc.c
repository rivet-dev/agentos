/* Test whether a basic getwc invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	wchar_t c = L'x';
	if ( fputwc(c, fp) == WEOF )
		err(1, "fputwc");
	if ( fflush(fp) == EOF )
		err(1, "fflush");
	if ( fseek(fp, 0, SEEK_SET) )
		err(1, "fseek");
	wint_t x = getwc(fp);
	if ( x == WEOF )
	{
		if ( feof(fp) )
			errx(1, "getwc: WEOF");
		err(1, "getwc");
	}
	if ( (wint_t) c != x )
		errx(1, "getwc got %lc instead of %lc", x, c);
	return 0;
}
