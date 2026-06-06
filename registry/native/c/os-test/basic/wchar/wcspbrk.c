/* Test whether a basic wcspbrk invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t buf[] = L"abcdefg";
	if ( wcspbrk(buf, L"eg") != buf + 4 )
		errx(1, "wcspbrk did not find 'e'");
	if ( wcspbrk(buf, L"x") )
		errx(1, "wcspbrk found absent character");
	return 0;
}
