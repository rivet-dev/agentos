/* Test whether a basic c32rtomb invocation works. */

#include <locale.h>
#include <uchar.h>

#include "../basic.h"

int main(void)
{
	if ( !setlocale(LC_CTYPE, "C.UTF-8") &&
	     !setlocale(LC_CTYPE, "POSIX.UTF-8") )
		errx(1, "no UTF-8 locale");

	const char* expected = "𐰀"; // U+10C00 OLD TURKIC LETTER ORKHON A

	mbstate_t ps = {0};
	char buf[MB_CUR_MAX];
	size_t amount = c32rtomb(buf, 0x10C00, &ps);
	if ( amount == (size_t) -1 )
		err(1, "c32rtomb");
	if ( amount != strlen(expected) )
		err(1, "c32rtomb(0x10C00) != strlen(expected)");
	if ( memcmp(buf, expected, strlen(expected)) != 0 )
		errx(1, "c16rtomb decoded incorrectly");

	return 0;
}
