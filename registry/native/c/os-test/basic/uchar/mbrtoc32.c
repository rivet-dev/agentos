/* Test whether a basic mbrtoc32 invocation works. */

#include <locale.h>
#include <uchar.h>

#include "../basic.h"

int main(void)
{
	if ( !setlocale(LC_CTYPE, "C.UTF-8") &&
	     !setlocale(LC_CTYPE, "POSIX.UTF-8") )
		errx(1, "no UTF-8 locale");

	const char* str = "𐰀"; // U+10C00 OLD TURKIC LETTER ORKHON A
	mbstate_t ps = {0};
	char32_t c32, expected;

	size_t amount = mbrtoc32(&c32, str, strlen(str), &ps);
	if ( amount == (size_t) -1 )
		err(1, "mbrtoc32");
	if ( amount == (size_t) -2 )
		errx(1, "mbrtoc32 was incomplete");
	if ( amount == (size_t) -3 )
		errx(1, "mbrtoc32 gave previous character");
	if ( amount == 0 )
		errx(1, "mbrtoc32 gave nul");
	if ( amount != strlen(str) )
		errx(1, "mbrtoc32 != strlen(str)");
	expected = 0x10C00;
	if ( c32 != expected )
		errx(1, "mbrtoc32 gave 0x%X, not 0x%X", c32, expected);

	return 0;
}
