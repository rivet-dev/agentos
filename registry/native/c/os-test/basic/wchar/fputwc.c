/* Test whether a basic fputwc invocation works. */

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
	return 0;
}
