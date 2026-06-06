/* Test whether a basic wcstok invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t buf[8] = L"abcdefg";
	wchar_t* saved;
	wchar_t* ptr = wcstok(buf, L"ce", &saved);
	if ( ptr != buf + 0 )
		errx(1, "first wcstok did not find ab");
	if ( wcscmp(ptr, L"ab") != 0 )
		errx(1, "first wcstok did not isolate ab");
	if ( wmemcmp(buf, L"ab\0defg", 8) != 0 )
		errx(1, "first wcstok left buffer in wrong state");
	ptr = wcstok(NULL, L"ce", &saved);
	if ( ptr != buf + 3 )
		errx(1, "second wcstok did not find d");
	if ( wcscmp(ptr, L"d") != 0 )
		errx(1, "second wcstok did not isolate d");
	if ( wmemcmp(buf, L"ab\0d\0fg", 8) != 0 )
		errx(1, "second wcstok left buffer in wrong state");
	ptr = wcstok(NULL, L"ce", &saved);
	if ( ptr != buf + 5 )
		errx(1, "third wcstok did not find fg");
	if ( wcscmp(ptr, L"fg") != 0 )
		errx(1, "third wcstok did not isolate fg");
	if ( wmemcmp(buf, L"ab\0d\0fg", 8) != 0 )
		errx(1, "third wcstok left buffer in wrong state");
	if ( wcstok(NULL, L"ce", &saved) )
		errx(1, "fourth wcstok found something");
	return 0;
}
