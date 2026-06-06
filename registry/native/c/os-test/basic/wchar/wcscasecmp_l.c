/* Test whether a basic wcscasecmp_l invocation works. */

#include <locale.h>
#include <wchar.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	if ( wcscasecmp_l(L"foo", L"FOO", locale) != 0 )
		errx(1, "wcscasecmp(\"foo\", \"FOO\") weren't equal");
	return 0;
}
