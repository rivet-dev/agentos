/* Test whether a basic localeconv invocation works. */

#include <locale.h>

#include "../basic.h"

int main(void)
{
	struct lconv* lconv = localeconv();
	if ( !lconv )
		errx(1, "localeconv returned NULL");
	return 0;
}
