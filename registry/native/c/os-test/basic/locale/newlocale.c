/* Test whether a basic newlocale invocation works. */

#include <locale.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = newlocale(LC_ALL_MASK, "C", (locale_t) 0);
	if ( locale == (locale_t) 0 )
		err(1, "newlocale");
	return 0;
}
