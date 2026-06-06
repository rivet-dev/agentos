/* Test whether a basic wcsrchr invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t buf[] = L"foo/bar/qux";
	if ( wcsrchr(buf, L'/') != buf + 7 )
		errx(1, "wcsrchr did not return last '/'");
	if ( wcsrchr(buf, L'X') )
		errx(1, "wcsrchr found absent character");
	return 0;
}
