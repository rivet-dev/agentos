/* Test whether a basic mbtowc invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc;
	int amount = mbtowc(&wc, "xy", 3);
	if ( amount < 0 )
		err(1, "mbtowc");
	if ( amount != 1 )
		err(1, "mbtowc was %d, not %d", amount, 1);
	if ( wc != L'x' )
		errx(1, "mbtowc decoded incorrectly");
	return 0;
}
