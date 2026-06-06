/* Test whether a basic swprintf invocation works. */

#include <wchar.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wformat-truncation"

int main(void)
{
	wchar_t buffer[10];
	int ret = swprintf(buffer, sizeof(buffer)/sizeof(buffer[0]),
	                   L"hello %ls %d", L"world", 42);
	if ( ret < 0 )
	{
		if ( errno != EOVERFLOW )
			err(1, "swprintf did not EOVERFLOW");
	}
	else
		errx(1, "swprinf succeeding instead of EOVERFLOW");
	ret = swprintf(buffer, sizeof(buffer)/sizeof(buffer[0]),
	               L"hello %ls", L"wor");
	if ( (size_t) ret != wcslen(L"hello wor") )
		err(1, "swprintf returned wrong length");
	const wchar_t* expected = L"hello wor";
	if ( wcscmp(buffer, expected) != 0 )
		err(1, "swprintf gave '%ls' instead of '%ls'", buffer, expected);
	return 0;
}
