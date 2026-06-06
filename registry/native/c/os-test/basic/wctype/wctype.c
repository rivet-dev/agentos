/* Test whether a basic wctype invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wctype_t charclass = wctype("alpha");
	if ( !charclass )
		errx(1, "wctype failed");
	return 0;
}
