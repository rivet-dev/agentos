/* Test whether a basic wcsftime invocation works. */

#include <time.h>
#include <wchar.h>

#include "../basic.h"

int main(void)
{
	struct tm tm =
	{
		.tm_year = 121,
		.tm_mon = 10,
		.tm_mday = 16,
		.tm_wday = 2,
		.tm_hour = 19,
		.tm_min = 58,
		.tm_sec = 28,
	};
	wchar_t buf[64];
	size_t length = wcsftime(buf, sizeof(buf), L"%Y-%m-%d %H:%M:%S", &tm);
	if ( !length )
		err(1, "wcsftime");
	const wchar_t* expected = L"2021-11-16 19:58:28";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "wcsftime gave %ls not %ls", buf, expected);
	return 0;
}
