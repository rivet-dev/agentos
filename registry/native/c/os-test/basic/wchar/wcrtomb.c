/* Test whether a basic wcrtomb invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	char mb[MB_CUR_MAX];
	size_t amount = wcrtomb(mb, L'A', &ps);
	if ( amount != 1 )
		errx(1, "wcrtomb() != -1");
	if ( mb[0] != 'A' )
		errx(1, "did not encode 'A'");
	return 0;
}
