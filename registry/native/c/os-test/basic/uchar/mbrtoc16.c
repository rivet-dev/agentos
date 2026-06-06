/* Test whether a basic mbrtoc16 invocation works. */

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
	char16_t c16, expected;

	size_t amount = mbrtoc16(&c16, str, strlen(str), &ps);
	if ( amount == (size_t) -1 )
		err(1, "first mbrtoc16");
	if ( amount == (size_t) -2 )
		errx(1, "first mbrtoc16 was incomplete");
	if ( amount == (size_t) -3 )
		errx(1, "first mbrtoc16 gave previous character");
	if ( amount == 0 )
		errx(1, "first mbrtoc16 gave nul");
	if ( amount != strlen(str) )
		errx(1, "first mbrtoc16 != strlen(str)");
	expected = 0xD803;
	if ( c16 != expected )
		errx(1, "first mbrtoc16 gave 0x%X, not 0x%X", c16, expected);

	amount = mbrtoc16(&c16, str + strlen(str), 0, &ps);
	if ( amount == (size_t) -1 )
		err(1, "second mbrtoc16");
	if ( amount == (size_t) -2 )
		errx(1, "second mbrtoc16 was incomplete");
	if ( amount == 0 )
		errx(1, "second mbrtoc16 gave nul");
	if ( amount != (size_t) -3 )
		errx(1, "second mbrtoc16 did not give previous character");
	expected = 0xDC00;
	if ( c16 != expected )
		errx(1, "first mbrtoc16 gave 0x%X, not 0x%X", c16, expected);

	return 0;
}
