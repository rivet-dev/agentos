/* Test whether a basic mbrtowc invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	wchar_t wc;
	size_t len = mbrtowc(&wc, "x", 1, &ps);
	if ( len != 1 )
		errx(1, "mbrtowc did not return 1");
	if ( wc != L'x' )
		errx(1, "mbrtowc did not give 'x'");
	return 0;
}
