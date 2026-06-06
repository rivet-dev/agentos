/* Test whether a basic iswgraph invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'x';
	wchar_t wc2 = L' ';
	if ( !iswgraph(wc1) )
		errx(1, "iswgraph('%lc') was not true", wc1);
	if ( iswgraph(wc2) )
		errx(1, "iswgraph('%lc') was not false", wc2);
	return 0;
}
