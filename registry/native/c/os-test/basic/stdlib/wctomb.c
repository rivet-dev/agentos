/* Test whether a basic wctomb invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char str[MB_CUR_MAX];
	int amount = wctomb(str, L'x');
	if ( amount < 0 )
		err(1, "wctomb");
	if ( amount != 1 )
		errx(1, "wctomb did not return 1");
	if ( str[0] != 'x' )
		errx(1, "wctomb did not provide 'x'");
	return 0;
}
