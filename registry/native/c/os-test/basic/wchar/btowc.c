/* Test whether a basic btowc invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wint_t value = btowc('A');
	if ( value != L'A' )
		errx(1, "btowc did not return 'A*");
	return 0;
}
