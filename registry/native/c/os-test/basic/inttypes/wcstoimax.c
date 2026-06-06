/* Test whether a basic wcstoimax invocation works. */

#include <inttypes.h>
#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	intmax_t value = wcstoimax(L"-4611686014132420609.1end", &end, 10);
	intmax_t expected = INTMAX_C(-4611686014132420609);
	if ( value != expected )
		errx(1, "wcstoimax returned %"PRIdMAX" rather than %"PRIdMAX"",
		     value, expected);
	if ( wcscmp(end, L".1end") != 0 )
		errx(1, "wcstoimax set wrong end pointer");
	return 0;
}
