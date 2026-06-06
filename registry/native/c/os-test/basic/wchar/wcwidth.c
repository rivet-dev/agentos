/*[XSI]*/
/* Test whether a basic wcwidth invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( wcwidth(L'A') != 1 )
		errx(1, "wcwidth(L'A') != 1");
	if ( wcwidth(L'\0') != 0 )
		errx(1, "wcwidth(L'\\0') != 0");
	if ( wcwidth(L'\n') != -1 )
		errx(1, "wcwidth(L'\\n') != -1");
	return 0;
}
