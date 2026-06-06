/* Test whether a basic open_wmemstream invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* buf;
	size_t size;
	FILE* fp = open_wmemstream(&buf, &size);
	if ( !fp )
		err(1, "open_wmemstream");
	if ( fflush(fp) == EOF )
		err(1, "first fflush");
	if ( !buf )
		errx(1, "second check: buf is NULL");
	if ( size != 0 )
		errx(1, "second check: size = %zu, expected %zu", size, 0);
	if ( fwprintf(fp, L"hello %ls %d", L"world", 42) < 0 )
		err(1, "first fwprintf");
	if ( fflush(fp) == EOF )
		err(1, "first fflush");
	if ( !buf )
		errx(1, "second check: buf is NULL");
	const wchar_t* expected1 = L"hello world 42";
	if ( size != wcslen(expected1) )
		errx(1, "second check: size = %zu, expected %zu", size, expected1);
	if ( wcscmp(buf, expected1) != 0 )
		err(1, "second check: buf is '%ls' instead of '%ls'", buf, expected1);
	if ( fwprintf(fp, L" cool") < 0 )
		err(1, "second fwprintf");
	if ( fclose(fp) == EOF )
		err(1, "fclose");
	const wchar_t* expected2 = L"hello world 42 cool";
	if ( size != wcslen(expected2) )
		errx(1, "second check: size = %zu, expected %zu", size, expected2);
	if ( wcscmp(buf, expected2) != 0 )
		err(1, "second check: buf is '%ls' instead of '%ls'", buf, expected2);
	free(buf);
	return 0;
}
