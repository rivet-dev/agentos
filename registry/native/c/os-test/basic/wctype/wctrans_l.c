/* Test whether a basic wctrans_l invocation works. */

#include <locale.h>
#include <wctype.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	wctrans_t desc = wctrans_l("tolower", locale);
	if ( !desc )
		err(1, "wctrans_l");
	return 0;
}
