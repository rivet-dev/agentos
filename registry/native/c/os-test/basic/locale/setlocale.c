/* Test whether a basic setlocale invocation works. */

#include <locale.h>

#include "../basic.h"

int main(void)
{
	char* locale = setlocale(LC_ALL, "C");
	if ( !locale )
		err(1, "setlocale");
	if ( !locale[0] )
		errx(1, "setlocale returned an empty string");
	return 0;
}
