/* Test whether a basic c16rtomb invocation works. */

#include <locale.h>
#include <uchar.h>

#include "../basic.h"

int main(void)
{
	if ( !setlocale(LC_CTYPE, "C.UTF-8") &&
	     !setlocale(LC_CTYPE, "POSIX.UTF-8") )
		errx(1, "no UTF-8 locale");

	mbstate_t ps = {0};
	char buf[MB_CUR_MAX];
	size_t amount = c16rtomb(buf, u'X', &ps);
	if ( amount == (size_t) -1 )
		err(1, "c16rtomb(u'X')");
	if ( amount != 1 )
		err(1, "c16rtomb(u'X') != 1");
	if ( buf[0] != 'X' )
		err(1, "c16rtomb(u'X') != 'X'");

	const char* expected = "𐰀"; // U+10C00 OLD TURKIC LETTER ORKHON A

	amount = c16rtomb(buf, 0xD803, &ps);
	if ( amount == (size_t) -1 )
		err(1, "c16rtomb(0xD803)");
	if ( amount != 0 )
		err(1, "c16rtomb(0xD803) != 0");

	amount = c16rtomb(buf, 0xDC00, &ps);
	if ( amount == (size_t) -1 )
		err(1, "c16rtomb(0xDC00)");
	if ( amount != strlen(expected) )
		err(1, "c16rtomb(0xDC00) != strlen(expected)");
	if ( memcmp(buf, expected, strlen(expected)) != 0 )
		errx(1, "c16rtomb decoded incorrectly");

	return 0;
}
