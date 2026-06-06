/* Test whether a basic wmemchr invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t buf[] = L"abcdefg";
	if ( wmemchr(buf, L'e', sizeof(buf)/sizeof(buf[0])) != buf + 4 )
		errx(1, "wmemchr did not return 'e'");
	if ( wmemchr(buf, L'x', sizeof(buf)/sizeof(buf[0])) )
		errx(1, "wmemchr found absent character");
	return 0;
}
